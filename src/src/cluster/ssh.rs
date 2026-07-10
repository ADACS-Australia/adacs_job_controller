use std::path::Path;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use russh::client;
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg};
use russh::{ChannelMsg, Disconnect};

use crate::config::clusters::ClusterConfig;

#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),
    #[error("Channel failed: {0}")]
    ChannelFailed(String),
    #[error("Command exited with status {0}")]
    CommandFailed(i32),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

impl From<russh::Error> for SshError {
    fn from(e: russh::Error) -> Self {
        Self::ConnectionFailed(e.to_string())
    }
}

struct SshHandler;

impl client::Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Run the remote `adacs_job_client` on a cluster via SSH.
///
/// # Errors
///
/// Returns `SshError` on connection, authentication, channel, or command failure.
pub async fn run_remote_client(config: &ClusterConfig, token: &str) -> Result<(), SshError> {
    tracing::info!(
        "SSH: Initiating connection to cluster '{}' at {}@{} (type: {})",
        config.name,
        config.username,
        config.host,
        config.connection_type
    );

    match config.connection_type.as_str() {
        "kerberos" => {
            tracing::debug!(
                "SSH: Using Kerberos authentication for cluster '{}'",
                config.name
            );
            run_via_kerberos(config, token).await
        }
        "ssh" => {
            tracing::debug!(
                "SSH: Using SSH key authentication for cluster '{}'",
                config.name
            );
            run_via_ssh(config, token).await
        }
        other => {
            tracing::warn!(
                "SSH: Unknown connection type '{}' for cluster '{}', defaulting to SSH",
                other,
                config.name
            );
            run_via_ssh(config, token).await
        }
    }
}

async fn run_via_ssh(config: &ClusterConfig, token: &str) -> Result<(), SshError> {
    tracing::debug!("SSH[{}]: Loading private key", config.name);
    let key_pair = load_private_key(&config.key).await?;
    tracing::trace!(
        "SSH[{}]: Private key loaded (algorithm: {:?})",
        config.name,
        key_pair.algorithm()
    );

    // No inactivity timeout: the remote adacs_job_client is a daemon that may
    // not produce continuous channel output. A timeout would kill the session.
    let cfg = Arc::new(client::Config {
        inactivity_timeout: None,
        ..<_>::default()
    });

    tracing::debug!("SSH[{}]: Connecting to {}:22", config.name, config.host);
    let mut session = client::connect(cfg, (&config.host[..], 22u16), SshHandler).await?;
    tracing::trace!("SSH[{}]: TCP connection established", config.name);

    tracing::debug!(
        "SSH[{}]: Authenticating user '{}' with public key",
        config.name,
        config.username
    );
    authenticate_public_key(&mut session, &config.username, key_pair).await?;
    tracing::info!("SSH[{}]: Authentication successful", config.name);

    let command = format!(
        "cd {} && source env.sh && ./adacs_job_client {}",
        config.path, token
    );
    tracing::trace!(
        "SSH[{}]: Executing remote command: cd {} && source env.sh && ./adacs_job_client [TOKEN]",
        config.name,
        config.path
    );

    let exit_code = execute_command(&mut session, &command).await?;
    tracing::debug!(
        "SSH[{}]: Remote command completed with exit code {}",
        config.name,
        exit_code
    );

    tracing::debug!("SSH[{}]: Disconnecting session", config.name);
    let _ = session
        .disconnect(Disconnect::ByApplication, "", "en")
        .await;

    if exit_code != 0 {
        tracing::warn!(
            "SSH[{}]: Command failed with exit code {}",
            config.name,
            exit_code
        );
        return Err(SshError::CommandFailed(exit_code.cast_signed()));
    }

    tracing::info!("SSH[{}]: Connection completed successfully", config.name);
    Ok(())
}

async fn load_private_key(key_source: &str) -> Result<PrivateKey, SshError> {
    let key_text = if Path::new(key_source).exists() {
        tokio::fs::read_to_string(key_source).await?
    } else {
        key_source.to_string()
    };

    russh::keys::decode_secret_key(&key_text, None)
        .map_err(|e| SshError::AuthenticationFailed(format!("Failed to parse key: {e}")))
}

async fn authenticate_public_key(
    session: &mut client::Handle<SshHandler>,
    username: &str,
    key_pair: PrivateKey,
) -> Result<(), SshError> {
    let key_pair = Arc::new(key_pair);
    let attempts = authentication_hash_attempts(
        key_pair.algorithm().is_rsa(),
        session.best_supported_rsa_hash().await?,
    );

    for hash_alg in attempts {
        let auth_res = session
            .authenticate_publickey(
                username,
                PrivateKeyWithHashAlg::new(Arc::clone(&key_pair), hash_alg),
            )
            .await?;

        if auth_res.success() {
            return Ok(());
        }
    }

    Err(SshError::AuthenticationFailed(
        "Public key authentication failed".to_string(),
    ))
}

#[allow(clippy::option_option)]
fn authentication_hash_attempts(
    is_rsa: bool,
    preferred_hash: Option<Option<HashAlg>>,
) -> Vec<Option<HashAlg>> {
    if !is_rsa {
        return vec![None];
    }

    let mut attempts = Vec::new();

    if let Some(hash_alg) = preferred_hash {
        attempts.push(hash_alg);
    }

    for fallback in [Some(HashAlg::Sha512), Some(HashAlg::Sha256), None] {
        if !attempts.contains(&fallback) {
            attempts.push(fallback);
        }
    }

    attempts
}

async fn run_via_kerberos(config: &ClusterConfig, token: &str) -> Result<(), SshError> {
    let tmp_dir = tempfile::TempDir::new()?;
    let keytab_path = tmp_dir.path().join("krb5.keytab");

    let keytab_content = if Path::new(&config.keytab).exists() {
        tokio::fs::read(&config.keytab).await?
    } else {
        STANDARD
            .decode(config.keytab.as_bytes())
            .map_err(|e| SshError::AuthenticationFailed(format!("Failed to decode keytab: {e}")))?
    };

    tokio::fs::write(&keytab_path, &keytab_content).await?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&keytab_path, std::fs::Permissions::from_mode(0o600))?;
    }

    // SAFETY: This is a controlled single-threaded context during startup.
    unsafe {
        std::env::set_var("KRB5_CLIENT_KTNAME", keytab_path.to_str().unwrap());
    }

    let principal = if config.kerberos_principal.is_empty() {
        &config.username
    } else {
        &config.kerberos_principal
    };

    let remote_cmd = format!(
        "cd {} && source env.sh && ./adacs_job_client {}",
        config.path, token
    );

    let output = tokio::process::Command::new("ssh")
        .args([
            "-o",
            "GSSAPIAuthentication=yes",
            "-o",
            "GSSAPIKeyExchange=yes",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "GSSAPIDelegateCredentials=no",
            "-l",
            principal,
            &config.host,
            &remote_cmd,
        ])
        .output()
        .await?;

    let exit_code = output.status.code().unwrap_or(-1);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("Kerberos SSH command failed (exit={exit_code}): {stderr}");
        return Err(SshError::CommandFailed(exit_code));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        tracing::info!("{line}");
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stderr.lines() {
        tracing::warn!("{line}");
    }

    Ok(())
}

async fn execute_command(
    session: &mut client::Handle<SshHandler>,
    command: &str,
) -> Result<u32, SshError> {
    let mut channel = session
        .channel_open_session()
        .await
        .map_err(|e| SshError::ChannelFailed(e.to_string()))?;

    channel
        .exec(true, command)
        .await
        .map_err(|e| SshError::ChannelFailed(e.to_string()))?;

    let mut exit_code: Option<u32> = None;

    loop {
        let Some(msg) = channel.wait().await else {
            break;
        };

        match msg {
            ChannelMsg::Data { data } => {
                let text = String::from_utf8_lossy(&data);
                tracing::info!("{text}");
            }
            ChannelMsg::ExtendedData { data, ext: 1 } => {
                let text = String::from_utf8_lossy(&data);
                tracing::warn!("{text}");
            }
            ChannelMsg::ExitStatus { exit_status } => {
                exit_code = Some(exit_status);
            }
            _ => {}
        }
    }

    exit_code.ok_or_else(|| {
        SshError::ChannelFailed("SSH channel closed without providing an exit status".to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::Algorithm;
    use std::fs;
    use tempfile::TempDir;

    const PKCS8_ED25519_KEY: &str = "-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEINTuctv5E1hK1bbY8fdp+K06/nwoy/HU++CXqI9EdVhC
-----END PRIVATE KEY-----";

    #[test]
    fn ssh_error_display_connection_failed() {
        let e = SshError::ConnectionFailed("timeout".to_string());
        assert_eq!(e.to_string(), "Connection failed: timeout");
    }

    #[test]
    fn ssh_error_display_auth_failed() {
        let e = SshError::AuthenticationFailed("bad key".to_string());
        assert_eq!(e.to_string(), "Authentication failed: bad key");
    }

    #[test]
    fn ssh_error_display_channel_failed() {
        let e = SshError::ChannelFailed("closed".to_string());
        assert_eq!(e.to_string(), "Channel failed: closed");
    }

    #[test]
    fn ssh_error_display_command_failed() {
        let e = SshError::CommandFailed(42);
        assert_eq!(e.to_string(), "Command exited with status 42");
    }

    #[test]
    fn ssh_error_display_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let e = SshError::IoError(io_err);
        assert_eq!(e.to_string(), "IO error: file missing");
    }

    #[test]
    fn run_remote_client_ssh_type_dispatches_to_ssh_path() {
        let config = ClusterConfig {
            name: "test".to_string(),
            host: "0.0.0.0".to_string(),
            username: "test".to_string(),
            path: "/tmp".to_string(),
            key: String::new(),
            connection_type: "ssh".to_string(),
            keytab: String::new(),
            kerberos_principal: String::new(),
            ltk: None,
        };
        // Should attempt SSH connection and fail (empty key is invalid)
        let result = run_remote_client(&config, "test-token");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt.block_on(result).unwrap_err();
        // Empty key fails during auth parsing
        assert!(matches!(err, SshError::AuthenticationFailed(_)));
    }

    #[test]
    fn run_remote_client_catch_all_dispatches_to_ssh() {
        let config = ClusterConfig {
            name: "test".to_string(),
            host: "0.0.0.0".to_string(),
            username: "test".to_string(),
            path: "/tmp".to_string(),
            key: String::new(),
            connection_type: "unknown_type".to_string(),
            keytab: String::new(),
            kerberos_principal: String::new(),
            ltk: None,
        };
        // Should treat unknown type as SSH (catch-all), not return an error
        let result = run_remote_client(&config, "test-token");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt.block_on(result).unwrap_err();
        // Empty key fails during auth parsing
        assert!(matches!(err, SshError::AuthenticationFailed(_)));
    }

    #[test]
    fn empty_key_returns_auth_error_not_panic() {
        let config = ClusterConfig {
            name: "test".to_string(),
            host: "0.0.0.0".to_string(),
            username: "test".to_string(),
            path: "/tmp".to_string(),
            key: String::new(),
            connection_type: "ssh".to_string(),
            keytab: String::new(),
            kerberos_principal: String::new(),
            ltk: None,
        };
        // With an empty key, parsing should produce an AuthFailed error,
        // not a panic or IO error
        let result = run_via_ssh(&config, "t");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt.block_on(result).unwrap_err();
        assert!(matches!(err, SshError::AuthenticationFailed(_)));
    }

    /// Run `ssh` path directly with a config that has a non-empty but invalid key.
    /// This validates the connection-attempt path without needing a real SSH server.
    #[test]
    fn invalid_key_returns_auth_error() {
        let config = ClusterConfig {
            name: "test".to_string(),
            host: "0.0.0.0".to_string(),
            username: "test".to_string(),
            path: "/tmp".to_string(),
            key: "not-a-real-key".to_string(),
            connection_type: "ssh".to_string(),
            keytab: String::new(),
            kerberos_principal: String::new(),
            ltk: None,
        };
        let result = run_via_ssh(&config, "t");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt.block_on(result).unwrap_err();
        assert!(matches!(err, SshError::AuthenticationFailed(_)));
    }

    #[test]
    fn authentication_hash_attempts_uses_single_none_for_non_rsa() {
        assert_eq!(authentication_hash_attempts(false, None), vec![None]);
    }

    #[test]
    fn authentication_hash_attempts_prefers_advertised_rsa_hash() {
        assert_eq!(
            authentication_hash_attempts(true, Some(Some(HashAlg::Sha256))),
            vec![Some(HashAlg::Sha256), Some(HashAlg::Sha512), None]
        );
    }

    #[test]
    fn authentication_hash_attempts_falls_back_across_rsa_algorithms() {
        assert_eq!(
            authentication_hash_attempts(true, None),
            vec![Some(HashAlg::Sha512), Some(HashAlg::Sha256), None]
        );
    }

    #[test]
    fn authentication_hash_attempts_keeps_legacy_when_server_only_advertises_ssh_rsa() {
        assert_eq!(
            authentication_hash_attempts(true, Some(None)),
            vec![None, Some(HashAlg::Sha512), Some(HashAlg::Sha256)]
        );
    }

    #[test]
    fn load_private_key_parses_openssh_private_key() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let parsed = rt.block_on(load_private_key(PKCS8_ED25519_KEY)).unwrap();

        assert_eq!(parsed.algorithm(), Algorithm::Ed25519);
    }

    #[test]
    fn load_private_key_parses_legacy_rsa_private_key() {
        let legacy_rsa_key = "-----BEGIN RSA PRIVATE KEY-----
MIIEpQIBAAKCAQEAw/FG8YLVoXhsUVZcWaY7iZekMxQ2TAfSVh0LTnRuzsumeLhb
0fh4scIt4C4MLwpGe/u3vj290C28jLkOtysqnIpB4iBUrFNRmEz2YuvjOzkFE8Ju
0l1VrTZ9APhpLZvzT2N7YmTXcLz1yWopCe4KqTHczEP4lfkothxEoACXMaxezt5o
wIYfagDaaH6jXJgJk1SQ5VYrROVpDjjX8/Zg01H1faFQUikYx0M8EwL1fY5B80Hd
6DYSok8kUZGfkZT8HQ54DBgocjSs449CVqkVoQC1aDB+LZpMWovY15q7hFgfQmYD
qulbZRWDxxogS6ui/zUR2IpX7wpQMKKkBS1qdQIDAQABAoIBAQCodpcCKfS2gSzP
uapowY1KvP/FkskkEU18EDiaWWyzi1AzVn5LRo+udT6wEacUAoebLU5K2BaMF+aW
Lr1CKnDWaeA/JIDoMDJk+TaU0i5pyppc5LwXTXvOEpzi6rCzL/O++88nR4AbQ7sm
Uom6KdksotwtGvttJe0ktaUi058qaoFZbels5Fwk5bM5GHDdV6De8uQjSfYV813P
tM/6A5rRVBjC5uY0ocBHxPXkqAdHfJuVk0uApjLrbm6k0M2dg1X5oyhDOf7ZIzAg
QGPgvtsVZkQlyrD1OoCMPwzgULPXTe8SktaP9EGvKdMf5kQOqUstqfyx+E4OZa0A
T82weLjBAoGBAOUChhaLQShL3Vsml/Nuhhw5LsxU7Li34QWM6P5AH0HMtsSncH8X
ULYcUKGbCmmMkVb7GtsrHa4ozy0fjq0Iq9cgufolytlvC0t1vKRsOY6poC2MQgaZ
bqRa05IKwhZdHTr9SUwB/ngtVNWRzzbFKLkn2W5oCpQGStAKqz3LbKstAoGBANsJ
EyrXPbWbG+QWzerCIi6shQl+vzOd3cxqWyWJVaZglCXtlyySV2eKWRW7TcVvaXQr
Nzm/99GNnux3pUCY6szy+9eevjFLLHbd+knzCZWKTZiWZWr503h/ztfFwrMzhoAh
z4nukD/OETugPvtG01c2sxZb/F8LH9KORznhlSlpAoGBAJnqg1J9j3JU4tZTbwcG
fo5ThHeCkINp2owPc70GPbvMqf4sBzjz46QyDaM//9SGzFwocplhNhaKiQvrzMnR
LSVucnCEm/xdXLr/y6S6tEiFCwnx3aJv1uQRw2bBYkcDmBTAjVXPdUcyOHU+BYXr
Jv6ioMlKlel8/SUsNoFWypeVAoGAXhr3Bjf1xlm+0O9PRyZjQ0RR4DN5eHbB/XpQ
cL8hclsaK3V5tuek79JL1f9kOYhVeVi74G7uzTSYbCY3dJp+ftGCjDAirNEMaIGU
cEMgAgSqs/0h06VESwg2WRQZQ57GkbR1E2DQzuj9FG4TwSe700OoC9o3gqon4PHJ
/j9CM8kCgYEAtPJf3xaeqtbiVVzpPAGcuPyajTzU0QHPrXEl8zr/+iSK4Thc1K+c
b9sblB+ssEUQD5IQkhTWcsXdslINQeL77WhIMZ2vBAH8Hcin4jgcLmwUZfpfnnFs
QaChXiDsryJZwsRnruvMRX9nedtqHrgnIsJLTXjppIhGhq5Kg4RQfOU=
-----END RSA PRIVATE KEY-----
";

        let rt = tokio::runtime::Runtime::new().unwrap();
        let parsed = rt.block_on(load_private_key(legacy_rsa_key)).unwrap();

        assert!(parsed.algorithm().is_rsa());
    }

    #[test]
    fn load_private_key_reads_key_file() {
        let tmp = TempDir::new().unwrap();
        let key_path = tmp.path().join("id_test");
        fs::write(&key_path, PKCS8_ED25519_KEY.as_bytes()).unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let parsed = rt
            .block_on(load_private_key(key_path.to_str().unwrap()))
            .unwrap();

        assert_eq!(parsed.algorithm(), Algorithm::Ed25519);
    }
}
