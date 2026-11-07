use std::path::Path;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use russh::client::{self, AuthResult, KeyboardInteractiveAuthResponse, Prompt};
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{ChannelMsg, Disconnect, MethodKind};

use crate::config::clusters::{CONNECTION_TYPE_KERBEROS, CONNECTION_TYPE_SSH, ClusterConfig};

/// SSH config options used for Kerberos-authenticated connections.
const GSSAPI_AUTH_OPTION: &str = "GSSAPIAuthentication=yes";
const GSSAPI_KEYEXCHANGE_OPTION: &str = "GSSAPIKeyExchange=yes";
const STRICT_HOST_KEY_CHECKING_OPTION: &str = "StrictHostKeyChecking=no";
const GSSAPI_DELEGATE_CREDENTIALS_OPTION: &str = "GSSAPIDelegateCredentials=no";

/// Maximum number of keyboard-interactive rounds answered before giving up.
///
/// The initial `authenticate_keyboard_interactive_start` request is not a
/// round: one round is one response sent to a set of server prompts. A real
/// multi-factor login needs one or two rounds; the cap only guards against a
/// misbehaving server that keeps prompting forever.
const MAX_KEYBOARD_INTERACTIVE_ROUNDS: usize = 8;

/// Errors from SSH or Kerberos remote-client connections to cluster hosts.
#[derive(Debug, thiserror::Error)]
pub enum SshError {
    /// TCP/SSH handshake or session setup failed before authentication.
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    /// SSH key or Kerberos credentials were rejected by the remote host.
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),
    /// Opening or using an SSH exec/shell channel failed.
    #[error("Channel failed: {0}")]
    ChannelFailed(String),
    /// The remote `adacs_job_client` command exited with a non-zero status.
    #[error("Command exited with status {0}")]
    CommandFailed(i32),
    /// The remote command was terminated by a signal rather than exiting.
    #[error("Command terminated by signal {0}")]
    CommandSignalled(String),
    /// Local I/O error while reading keys or streaming channel data.
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

impl From<russh::Error> for SshError {
    fn from(e: russh::Error) -> Self {
        Self::ConnectionFailed(e.to_string())
    }
}

/// The RSA hash algorithm the SSH server advertised for public-key auth.
#[derive(Clone, Copy)]
enum PreferredRsaHash {
    /// Server did not advertise an RSA hash algorithm.
    NotAdvertised,
    /// Server only advertises legacy `ssh-rsa` (SHA-1).
    LegacySshRsa,
    /// Server advertises a specific RSA hash algorithm.
    Specific(HashAlg),
}

impl From<Option<Option<HashAlg>>> for PreferredRsaHash {
    fn from(value: Option<Option<HashAlg>>) -> Self {
        match value {
            None => Self::NotAdvertised,
            Some(None) => Self::LegacySshRsa,
            Some(Some(hash_alg)) => Self::Specific(hash_alg),
        }
    }
}

struct SshHandler;

impl client::Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        tracing::info!(
            "SSH: Accepted server host key ({})",
            format_server_key(&server_public_key.public_key())
        );
        Ok(true)
    }
}

/// Format a server host key's algorithm and SHA-256 fingerprint for logging.
fn format_server_key(key: &russh::keys::PublicKey) -> String {
    format!("{} {}", key.algorithm(), key.fingerprint(HashAlg::Sha256))
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
        CONNECTION_TYPE_KERBEROS => {
            tracing::debug!(
                "SSH: Using Kerberos authentication for cluster '{}'",
                config.name
            );
            run_via_kerberos(config, token).await
        }
        CONNECTION_TYPE_SSH => {
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

/// Build the remote `adacs_job_client` command executed on the cluster host.
///
/// Shared by the SSH and Kerberos connection paths so the command string
/// (working directory, environment setup, binary name, token) stays in one place.
fn build_remote_client_command(path: &str, token: &str) -> String {
    format!(
        "cd {} && source env.sh && ./adacs_job_client {}",
        shell_quote(path),
        shell_quote(token)
    )
}

/// Wrap a value in single quotes so it is passed literally to the remote shell,
/// escaping any embedded single quote as `'\''`.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
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

    let command = build_remote_client_command(&config.path, token);
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
    if let Err(e) = session
        .disconnect(Disconnect::ByApplication, "", "en")
        .await
    {
        tracing::warn!("SSH[{}]: Failed to disconnect session: {e}", config.name);
    }

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

/// True when the server accepted the public key as the *first* factor but still
/// requires the keyboard-interactive round to finish the login.
///
/// Servers configured with
/// `AuthenticationMethods publickey,keyboard-interactive:pam` answer the
/// public-key attempt with `partial_success = true` and advertise
/// keyboard-interactive as the remaining method. A TOTP exemption only removes
/// the prompt *inside* that round, not the round itself.
fn needs_keyboard_interactive(result: &AuthResult) -> bool {
    matches!(
        result,
        AuthResult::Failure {
            remaining_methods,
            partial_success: true,
        } if remaining_methods.contains(&MethodKind::KeyboardInteractive)
    )
}

/// One empty response per prompt, as required by RFC 4256.
///
/// Every prompt must be answered, but an exempt key has nothing to type into
/// the second-factor prompt, so the answers are intentionally empty.
fn empty_responses(prompts: &[Prompt]) -> Vec<String> {
    vec![String::new(); prompts.len()]
}

/// Complete the keyboard-interactive phase of a multi-factor login.
///
/// # Errors
///
/// Returns `SshError::AuthenticationFailed` when the round is rejected, when it
/// keeps prompting past [`MAX_KEYBOARD_INTERACTIVE_ROUNDS`], or on a transport
/// error while starting or answering the round.
async fn complete_keyboard_interactive(
    session: &mut client::Handle<SshHandler>,
    username: &str,
) -> Result<(), SshError> {
    let mut response = session
        .authenticate_keyboard_interactive_start(username, None)
        .await
        .map_err(|e| {
            SshError::AuthenticationFailed(format!("keyboard-interactive start failed: {e}"))
        })?;

    let mut rounds = 0;
    loop {
        match response {
            KeyboardInteractiveAuthResponse::Success => {
                tracing::info!("SSH: keyboard-interactive authentication completed");
                return Ok(());
            }
            KeyboardInteractiveAuthResponse::Failure {
                remaining_methods,
                partial_success,
            } => {
                return Err(SshError::AuthenticationFailed(format!(
                    "keyboard-interactive rejected (partial success: {partial_success}, \
                     server continues with: {remaining_methods:?})"
                )));
            }
            KeyboardInteractiveAuthResponse::InfoRequest {
                name,
                instructions,
                prompts,
            } => {
                rounds += 1;
                if rounds > MAX_KEYBOARD_INTERACTIVE_ROUNDS {
                    return Err(SshError::AuthenticationFailed(format!(
                        "keyboard-interactive did not finish after \
                         {MAX_KEYBOARD_INTERACTIVE_ROUNDS} rounds"
                    )));
                }
                if !prompts.is_empty() {
                    tracing::warn!(
                        "SSH: keyboard-interactive prompts received ({name}: {instructions}); \
                         answering {} prompt(s) with empty responses",
                        prompts.len()
                    );
                }
                response = session
                    .authenticate_keyboard_interactive_respond(empty_responses(&prompts))
                    .await
                    .map_err(|e| {
                        SshError::AuthenticationFailed(format!(
                            "keyboard-interactive response failed: {e}"
                        ))
                    })?;
            }
        }
    }
}

async fn authenticate_public_key(
    session: &mut client::Handle<SshHandler>,
    username: &str,
    key_pair: PrivateKey,
) -> Result<(), SshError> {
    let key_pair = Arc::new(key_pair);
    let attempts = authentication_hash_attempts(
        key_pair.algorithm().is_rsa(),
        session.best_supported_rsa_hash().await?.into(),
    );
    let mut last_failure = None;

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

        // The key was accepted as the first factor: finish the multi-factor
        // login now. This must happen inside the loop, otherwise an RSA key
        // would retry the remaining hash algorithms before reaching the
        // second factor.
        if needs_keyboard_interactive(&auth_res) {
            tracing::info!(
                "SSH: public key accepted as first factor; completing keyboard-interactive second factor"
            );
            return complete_keyboard_interactive(session, username).await;
        }

        if let AuthResult::Failure {
            remaining_methods,
            partial_success,
        } = &auth_res
        {
            last_failure = Some(format!(
                "Public key authentication failed (partial success: {partial_success}, \
                 server continues with: {remaining_methods:?})"
            ));
        }
    }

    Err(SshError::AuthenticationFailed(last_failure.unwrap_or_else(
        || "Public key authentication failed".to_string(),
    )))
}

fn authentication_hash_attempts(
    is_rsa: bool,
    preferred_hash: PreferredRsaHash,
) -> Vec<Option<HashAlg>> {
    if !is_rsa {
        return vec![None];
    }

    let mut attempts = Vec::new();

    match preferred_hash {
        PreferredRsaHash::NotAdvertised => {}
        PreferredRsaHash::LegacySshRsa => attempts.push(None),
        PreferredRsaHash::Specific(hash_alg) => attempts.push(Some(hash_alg)),
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

    let principal = if config.kerberos_principal.is_empty() {
        &config.username
    } else {
        &config.kerberos_principal
    };

    let remote_cmd = build_remote_client_command(&config.path, token);

    let output = build_kerberos_ssh_command(principal, &config.host, &remote_cmd, &keytab_path)
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

/// Environment variable that points `ssh` at the Kerberos client keytab.
const KRB5_CLIENT_KTNAME: &str = "KRB5_CLIENT_KTNAME";

/// Build the `ssh` command used for Kerberos-authenticated connections.
///
/// The `KRB5_CLIENT_KTNAME` environment variable is scoped to the child
/// process via [`tokio::process::Command::env`] rather than set globally,
/// so it never leaks into the server's own environment.
fn build_kerberos_ssh_command(
    principal: &str,
    host: &str,
    remote_cmd: &str,
    keytab_path: &Path,
) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("ssh");
    cmd.args([
        "-o",
        GSSAPI_AUTH_OPTION,
        "-o",
        GSSAPI_KEYEXCHANGE_OPTION,
        "-o",
        STRICT_HOST_KEY_CHECKING_OPTION,
        "-o",
        GSSAPI_DELEGATE_CREDENTIALS_OPTION,
        "-l",
        principal,
        host,
        remote_cmd,
    ])
    .env(KRB5_CLIENT_KTNAME, keytab_path);
    cmd
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
            other => {
                if let Some(code) = handle_channel_msg(other)? {
                    exit_code = Some(code);
                }
            }
        }
    }

    exit_code.ok_or_else(|| {
        SshError::ChannelFailed("SSH channel closed without providing an exit status".to_string())
    })
}

/// Process one message from the remote command's channel.
///
/// Returns `Ok(Some(exit_status))` when the command reported an exit status,
/// `Ok(None)` for messages that carry no termination information, and `Err`
/// when the command was terminated by a signal (RFC 4254) so the caller can
/// report the signal instead of a misleading generic failure.
fn handle_channel_msg(msg: ChannelMsg) -> Result<Option<u32>, SshError> {
    match msg {
        ChannelMsg::ExitStatus { exit_status } => Ok(Some(exit_status)),
        ChannelMsg::ExitSignal { signal_name, .. } => {
            Err(SshError::CommandSignalled(format!("{signal_name:?}")))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::MethodSet;
    use russh::keys::Algorithm;
    use russh::server::Server as _;
    use std::borrow::Cow;
    use std::fs;
    use std::time::Duration;
    use tempfile::TempDir;

    const PKCS8_ED25519_KEY: &str = "-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEINTuctv5E1hK1bbY8fdp+K06/nwoy/HU++CXqI9EdVhC
-----END PRIVATE KEY-----";

    fn test_cluster_config(key: &str, connection_type: &str) -> ClusterConfig {
        ClusterConfig {
            name: "test".to_string(),
            host: "0.0.0.0".to_string(),
            username: "test".to_string(),
            path: "/tmp".to_string(),
            key: key.to_string(),
            connection_type: connection_type.to_string(),
            keytab: String::new(),
            kerberos_principal: String::new(),
            ltk: None,
        }
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Runtime::new().unwrap().block_on(future)
    }

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
    fn ssh_error_display_command_signalled() {
        let e = SshError::CommandSignalled("KILL".to_string());
        assert_eq!(e.to_string(), "Command terminated by signal KILL");
    }

    #[test]
    fn handle_channel_msg_reports_exit_signal() {
        let err = handle_channel_msg(ChannelMsg::ExitSignal {
            signal_name: russh::Sig::KILL,
            core_dumped: false,
            error_message: String::new(),
            lang_tag: String::new(),
        })
        .unwrap_err();

        assert!(
            matches!(&err, SshError::CommandSignalled(name) if name == "KILL"),
            "signal should be reported by name: {err:?}"
        );
    }

    #[test]
    fn handle_channel_msg_returns_exit_status() {
        let result = handle_channel_msg(ChannelMsg::ExitStatus { exit_status: 7 });
        assert_eq!(result.unwrap(), Some(7));
    }

    #[test]
    fn handle_channel_msg_ignores_data() {
        let result = handle_channel_msg(ChannelMsg::Data {
            data: bytes::Bytes::from_static(b"hello"),
        });
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn ssh_error_display_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let e = SshError::IoError(io_err);
        assert_eq!(e.to_string(), "IO error: file missing");
    }

    #[test]
    fn run_remote_client_ssh_type_dispatches_to_ssh_path() {
        let config = test_cluster_config("", "ssh");
        // Should attempt SSH connection and fail (empty key is invalid)
        let result = run_remote_client(&config, "test-token");
        let err = block_on(result).unwrap_err();
        // Empty key fails during auth parsing
        assert!(matches!(err, SshError::AuthenticationFailed(_)));
    }

    #[test]
    fn run_remote_client_catch_all_dispatches_to_ssh() {
        let config = test_cluster_config("", "unknown_type");
        // Should treat unknown type as SSH (catch-all), not return an error
        let result = run_remote_client(&config, "test-token");
        let err = block_on(result).unwrap_err();
        // Empty key fails during auth parsing
        assert!(matches!(err, SshError::AuthenticationFailed(_)));
    }

    #[test]
    fn empty_key_returns_auth_error_not_panic() {
        let config = test_cluster_config("", "ssh");
        // With an empty key, parsing should produce an AuthFailed error,
        // not a panic or IO error
        let result = run_via_ssh(&config, "t");
        let err = block_on(result).unwrap_err();
        assert!(matches!(err, SshError::AuthenticationFailed(_)));
    }

    /// Run `ssh` path directly with a config that has a non-empty but invalid key.
    /// This validates the connection-attempt path without needing a real SSH server.
    #[test]
    fn invalid_key_returns_auth_error() {
        let config = test_cluster_config("not-a-real-key", "ssh");
        let result = run_via_ssh(&config, "t");
        let err = block_on(result).unwrap_err();
        assert!(matches!(err, SshError::AuthenticationFailed(_)));
    }

    #[test]
    fn preferred_rsa_hash_from_none_is_not_advertised() {
        let hash = PreferredRsaHash::from(None);
        assert!(matches!(hash, PreferredRsaHash::NotAdvertised));
    }

    #[test]
    fn preferred_rsa_hash_from_some_none_is_legacy_ssh_rsa() {
        let hash = PreferredRsaHash::from(Some(None));
        assert!(matches!(hash, PreferredRsaHash::LegacySshRsa));
    }

    #[test]
    fn preferred_rsa_hash_from_some_some_is_specific() {
        let hash = PreferredRsaHash::from(Some(Some(HashAlg::Sha256)));
        assert!(matches!(hash, PreferredRsaHash::Specific(HashAlg::Sha256)));
    }

    #[test]
    fn authentication_hash_attempts_uses_single_none_for_non_rsa() {
        assert_eq!(
            authentication_hash_attempts(false, PreferredRsaHash::NotAdvertised),
            vec![None]
        );
    }

    #[test]
    fn authentication_hash_attempts_prefers_advertised_rsa_hash() {
        assert_eq!(
            authentication_hash_attempts(true, PreferredRsaHash::Specific(HashAlg::Sha256)),
            vec![Some(HashAlg::Sha256), Some(HashAlg::Sha512), None]
        );
    }

    #[test]
    fn authentication_hash_attempts_falls_back_across_rsa_algorithms() {
        assert_eq!(
            authentication_hash_attempts(true, PreferredRsaHash::NotAdvertised),
            vec![Some(HashAlg::Sha512), Some(HashAlg::Sha256), None]
        );
    }

    #[test]
    fn authentication_hash_attempts_keeps_legacy_when_server_only_advertises_ssh_rsa() {
        assert_eq!(
            authentication_hash_attempts(true, PreferredRsaHash::LegacySshRsa),
            vec![None, Some(HashAlg::Sha512), Some(HashAlg::Sha256)]
        );
    }

    #[test]
    fn needs_keyboard_interactive_true_for_partial_keyboard_interactive_failure() {
        let result = AuthResult::Failure {
            remaining_methods: MethodSet::from(&[MethodKind::KeyboardInteractive][..]),
            partial_success: true,
        };

        assert!(needs_keyboard_interactive(&result));
    }

    #[test]
    fn needs_keyboard_interactive_false_on_success() {
        assert!(!needs_keyboard_interactive(&AuthResult::Success));
    }

    #[test]
    fn needs_keyboard_interactive_false_for_outright_rejection() {
        let result = AuthResult::Failure {
            remaining_methods: MethodSet::from(&[MethodKind::PublicKey, MethodKind::Password][..]),
            partial_success: false,
        };

        assert!(!needs_keyboard_interactive(&result));
    }

    #[test]
    fn needs_keyboard_interactive_false_when_only_other_methods_remain() {
        let result = AuthResult::Failure {
            remaining_methods: MethodSet::from(&[MethodKind::Password][..]),
            partial_success: true,
        };

        assert!(!needs_keyboard_interactive(&result));
    }

    #[test]
    fn empty_responses_returns_one_empty_string_per_prompt() {
        let prompts = [
            Prompt {
                prompt: "Password: ".to_string(),
                echo: false,
            },
            Prompt {
                prompt: "Verification code: ".to_string(),
                echo: false,
            },
            Prompt {
                prompt: "Token: ".to_string(),
                echo: true,
            },
        ];

        assert_eq!(
            empty_responses(&prompts),
            vec![String::new(), String::new(), String::new()]
        );
    }

    #[test]
    fn empty_responses_is_empty_without_prompts() {
        assert!(empty_responses(&[]).is_empty());
    }

    #[test]
    fn load_private_key_parses_openssh_private_key() {
        let parsed = block_on(load_private_key(PKCS8_ED25519_KEY)).unwrap();

        assert_eq!(parsed.algorithm(), Algorithm::Ed25519);
    }

    #[test]
    fn format_server_key_contains_algorithm_and_sha256_fingerprint() {
        let key = block_on(load_private_key(PKCS8_ED25519_KEY)).unwrap();
        let formatted = format_server_key(key.public_key());
        assert!(
            formatted.starts_with("ssh-ed25519 SHA256:"),
            "unexpected format: {formatted}"
        );
    }

    /// Legacy PKCS#1 RSA private key, used to exercise the RSA code paths.
    const LEGACY_RSA_KEY: &str = "-----BEGIN RSA PRIVATE KEY-----
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

    #[test]
    fn load_private_key_parses_legacy_rsa_private_key() {
        let parsed = block_on(load_private_key(LEGACY_RSA_KEY)).unwrap();

        assert!(parsed.algorithm().is_rsa());
    }

    #[test]
    fn load_private_key_reads_key_file() {
        let tmp = TempDir::new().unwrap();
        let key_path = tmp.path().join("id_test");
        fs::write(&key_path, PKCS8_ED25519_KEY.as_bytes()).unwrap();

        let parsed = block_on(load_private_key(key_path.to_str().unwrap())).unwrap();

        assert_eq!(parsed.algorithm(), Algorithm::Ed25519);
    }

    #[test]
    fn build_remote_client_command_constructs_expected_string() {
        assert_eq!(
            build_remote_client_command("/srv/adacs", "secret-token"),
            "cd '/srv/adacs' && source env.sh && ./adacs_job_client 'secret-token'"
        );
    }

    #[test]
    fn build_remote_client_command_quotes_path_with_spaces() {
        assert_eq!(
            build_remote_client_command("/srv/adacs cluster", "secret-token"),
            "cd '/srv/adacs cluster' && source env.sh && ./adacs_job_client 'secret-token'"
        );
    }

    #[test]
    fn kerberos_keytab_env_is_scoped_to_child_command() {
        // SAFETY: Test-only cleanup to guarantee a clean baseline; the test
        // process is single-threaded at this point.
        unsafe {
            std::env::remove_var(KRB5_CLIENT_KTNAME);
        }

        let keytab_path = Path::new("/tmp/fake-krb5.keytab");
        let cmd = build_kerberos_ssh_command("user", "host", "remote", keytab_path);

        let envs: Vec<_> = cmd.as_std().get_envs().collect();
        assert!(
            envs.iter()
                .any(|(k, v)| { *k == KRB5_CLIENT_KTNAME && *v == Some(keytab_path.as_os_str()) }),
            "KRB5_CLIENT_KTNAME must be set on the child ssh command"
        );

        assert!(
            std::env::var_os(KRB5_CLIENT_KTNAME).is_none(),
            "KRB5_CLIENT_KTNAME must not leak into the global process environment"
        );
    }

    #[test]
    fn kerberos_ssh_command_includes_gssapi_flags_and_target() {
        let keytab_path = Path::new("/tmp/fake-krb5.keytab");
        let cmd = build_kerberos_ssh_command(
            "alice",
            "cluster.example.com",
            "cd /srv && ./adacs_job_client",
            keytab_path,
        );

        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();

        for expected in [
            "-o",
            GSSAPI_AUTH_OPTION,
            "-o",
            GSSAPI_KEYEXCHANGE_OPTION,
            "-o",
            STRICT_HOST_KEY_CHECKING_OPTION,
            "-o",
            GSSAPI_DELEGATE_CREDENTIALS_OPTION,
            "-l",
            "alice",
            "cluster.example.com",
            "cd /srv && ./adacs_job_client",
        ] {
            assert!(
                args.iter().any(|a| a == expected),
                "expected {expected:?} in args {args:?}"
            );
        }
    }

    // ========================================================================
    // Multi-factor authentication tests
    //
    // Login nodes enforce `AuthenticationMethods publickey,keyboard-interactive:pam`:
    // the public key is only the *first* factor and the client must complete a
    // keyboard-interactive round. These tests drive `authenticate_public_key`
    // against an in-process russh server configured with the same policy.
    // ========================================================================

    /// Timeout applied to every client interaction so a harness bug fails fast
    /// instead of hanging the suite.
    const AUTH_TEST_TIMEOUT: Duration = Duration::from_secs(10);

    /// What the test server observed during authentication.
    #[derive(Debug, Default)]
    struct AuthLog {
        publickey_attempts: usize,
        keyboard_interactive_attempts: usize,
        keyboard_interactive_responses: Vec<String>,
    }

    /// How the test server answers the public-key step.
    #[derive(Clone, Copy)]
    enum PublicKeyOutcome {
        /// Key rejected outright (not a multi-factor server).
        Rejected,
        /// Key accepted as the first factor; keyboard-interactive remains.
        PartialKeyboardInteractive,
        /// Key accepted as the first factor, but another method remains.
        PartialPasswordOnly,
    }

    /// How the test server answers the keyboard-interactive round.
    #[derive(Clone, Copy)]
    enum KeyboardInteractiveOutcome {
        /// Accept once the prompts have been answered (or none were asked).
        Accept,
        /// Keep prompting after every response (round-cap test).
        RepeatPrompts,
        /// Reject the answers, as a non-exempt key's failed second factor.
        Reject,
    }

    /// Test server mirroring the login-node multi-factor policy.
    #[derive(Clone)]
    struct MfaTestServer {
        log: Arc<std::sync::Mutex<AuthLog>>,
        /// Prompts issued in the keyboard-interactive round. Empty means the
        /// key is exempt and the round carries no questions.
        prompts: Vec<String>,
        outcome: PublicKeyOutcome,
        keyboard_interactive_outcome: KeyboardInteractiveOutcome,
    }

    impl russh::server::Server for MfaTestServer {
        type Handler = MfaTestHandler;

        fn new_client(&mut self, _peer_addr: Option<std::net::SocketAddr>) -> Self::Handler {
            MfaTestHandler {
                log: Arc::clone(&self.log),
                prompts: self.prompts.clone(),
                outcome: self.outcome,
                keyboard_interactive_outcome: self.keyboard_interactive_outcome,
            }
        }
    }

    struct MfaTestHandler {
        log: Arc<std::sync::Mutex<AuthLog>>,
        prompts: Vec<String>,
        outcome: PublicKeyOutcome,
        keyboard_interactive_outcome: KeyboardInteractiveOutcome,
    }

    impl russh::server::Handler for MfaTestHandler {
        type Error = russh::Error;

        async fn auth_publickey(
            &mut self,
            _user: &str,
            _key: russh::server::VerifiedPublicKey<'_>,
        ) -> Result<russh::server::Auth, Self::Error> {
            if let Ok(mut log) = self.log.lock() {
                log.publickey_attempts += 1;
            }

            let auth = match self.outcome {
                PublicKeyOutcome::Rejected => russh::server::Auth::Reject {
                    proceed_with_methods: Some(MethodSet::from(
                        &[MethodKind::PublicKey, MethodKind::KeyboardInteractive][..],
                    )),
                    partial_success: false,
                },
                PublicKeyOutcome::PartialKeyboardInteractive => russh::server::Auth::Reject {
                    proceed_with_methods: Some(MethodSet::from(
                        &[MethodKind::KeyboardInteractive][..],
                    )),
                    partial_success: true,
                },
                PublicKeyOutcome::PartialPasswordOnly => russh::server::Auth::Reject {
                    proceed_with_methods: Some(MethodSet::from(&[MethodKind::Password][..])),
                    partial_success: true,
                },
            };

            Ok(auth)
        }

        async fn auth_keyboard_interactive<'a>(
            &'a mut self,
            _user: &str,
            _submethods: &str,
            response: Option<russh::server::Response<'a>>,
        ) -> Result<russh::server::Auth, Self::Error> {
            let answers: Option<Vec<String>> = response.map(|responses| {
                responses
                    .map(|raw| String::from_utf8_lossy(&raw).into_owned())
                    .collect()
            });
            let answered = answers.is_some();

            if let Ok(mut log) = self.log.lock() {
                log.keyboard_interactive_attempts += 1;
                if let Some(answers) = answers {
                    log.keyboard_interactive_responses = answers;
                }
            }

            match self.keyboard_interactive_outcome {
                KeyboardInteractiveOutcome::Reject if answered => {
                    return Ok(russh::server::Auth::Reject {
                        proceed_with_methods: Some(MethodSet::from(
                            &[MethodKind::KeyboardInteractive][..],
                        )),
                        partial_success: false,
                    });
                }
                KeyboardInteractiveOutcome::Accept if answered || self.prompts.is_empty() => {
                    return Ok(russh::server::Auth::Accept);
                }
                _ => {}
            }

            let prompts: Vec<(Cow<'static, str>, bool)> = self
                .prompts
                .iter()
                .cloned()
                .map(|prompt| (Cow::Owned(prompt), false))
                .collect();

            Ok(russh::server::Auth::Partial {
                name: Cow::Borrowed(""),
                instructions: Cow::Borrowed(""),
                prompts: Cow::Owned(prompts),
            })
        }
    }

    /// An in-process server bound to an ephemeral loopback port.
    struct TestServer {
        addr: std::net::SocketAddr,
        log: Arc<std::sync::Mutex<AuthLog>>,
        _task: tokio::task::JoinHandle<()>,
    }

    async fn start_mfa_test_server(
        prompts: Vec<String>,
        outcome: PublicKeyOutcome,
        keyboard_interactive_outcome: KeyboardInteractiveOutcome,
    ) -> TestServer {
        let host_key = ed25519_test_key();

        let config = Arc::new(russh::server::Config {
            keys: vec![host_key],
            methods: MethodSet::from(&[MethodKind::PublicKey, MethodKind::KeyboardInteractive][..]),
            // Keep the constant-time rejection delay short so tests stay fast.
            auth_rejection_time: Duration::from_millis(10),
            auth_rejection_time_initial: Some(Duration::ZERO),
            inactivity_timeout: Some(AUTH_TEST_TIMEOUT),
            ..Default::default()
        });

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("test server address");

        let log = Arc::new(std::sync::Mutex::new(AuthLog::default()));
        let mut server = MfaTestServer {
            log: Arc::clone(&log),
            prompts,
            outcome,
            keyboard_interactive_outcome,
        };

        let task = tokio::spawn(async move {
            let _ = server.run_on_socket(config, &listener).await;
        });

        TestServer {
            addr,
            log,
            _task: task,
        }
    }

    fn ed25519_test_key() -> PrivateKey {
        russh::keys::decode_secret_key(PKCS8_ED25519_KEY, None).expect("committed ed25519 test key")
    }

    fn rsa_test_key() -> PrivateKey {
        russh::keys::decode_secret_key(LEGACY_RSA_KEY, None).expect("committed RSA test key")
    }

    /// Connect to the test server and run the real public-key authentication
    /// path against it, with a timeout on both steps.
    async fn authenticate_against_test_server(
        server: &TestServer,
        key: PrivateKey,
    ) -> Result<(), SshError> {
        let cfg = Arc::new(client::Config {
            inactivity_timeout: None,
            ..<_>::default()
        });

        let mut session = tokio::time::timeout(
            AUTH_TEST_TIMEOUT,
            client::connect(cfg, ("127.0.0.1", server.addr.port()), SshHandler),
        )
        .await
        .expect("connection to the test server timed out")?;

        tokio::time::timeout(
            AUTH_TEST_TIMEOUT,
            authenticate_public_key(&mut session, "testuser", key),
        )
        .await
        .expect("authentication against the test server timed out")
    }

    fn auth_log(server: &TestServer) -> std::sync::MutexGuard<'_, AuthLog> {
        server.log.lock().expect("auth log mutex")
    }

    #[tokio::test]
    async fn connect_completes_keyboard_interactive_after_partial_publickey() {
        let server = start_mfa_test_server(
            Vec::new(),
            PublicKeyOutcome::PartialKeyboardInteractive,
            KeyboardInteractiveOutcome::Accept,
        )
        .await;

        let result = authenticate_against_test_server(&server, ed25519_test_key()).await;
        assert!(
            result.is_ok(),
            "authentication should survive the keyboard-interactive round: {result:?}"
        );

        let log = auth_log(&server);
        assert_eq!(
            log.publickey_attempts, 1,
            "public key should be offered once"
        );
        assert_eq!(
            log.keyboard_interactive_attempts, 1,
            "client must run the keyboard-interactive round"
        );
        assert!(
            log.keyboard_interactive_responses.is_empty(),
            "no prompts should mean no responses: {:?}",
            log.keyboard_interactive_responses
        );
    }

    #[tokio::test]
    async fn connect_answers_keyboard_interactive_prompts_with_empty_responses() {
        let server = start_mfa_test_server(
            vec!["Verification code: ".to_string()],
            PublicKeyOutcome::PartialKeyboardInteractive,
            KeyboardInteractiveOutcome::Accept,
        )
        .await;

        let result = authenticate_against_test_server(&server, ed25519_test_key()).await;
        assert!(
            result.is_ok(),
            "authentication should answer the prompt and finish: {result:?}"
        );

        let log = auth_log(&server);
        assert_eq!(
            log.keyboard_interactive_attempts, 2,
            "one round to receive the prompt, one to answer it"
        );
        assert_eq!(
            log.keyboard_interactive_responses,
            vec![String::new()],
            "prompts are answered with empty strings"
        );
    }

    #[tokio::test]
    async fn connect_reports_partial_publickey_failure_when_keyboard_interactive_is_not_offered() {
        let server = start_mfa_test_server(
            Vec::new(),
            PublicKeyOutcome::PartialPasswordOnly,
            KeyboardInteractiveOutcome::Accept,
        )
        .await;

        let result = authenticate_against_test_server(&server, ed25519_test_key()).await;
        let error = result.expect_err("authentication should fail when another method remains");
        let message = error.to_string();

        assert!(
            message.contains("partial success: true"),
            "error should report the partial success: {message}"
        );
        assert!(
            message.contains("Password"),
            "error should name the advertised method: {message}"
        );

        let log = auth_log(&server);
        assert_eq!(
            log.keyboard_interactive_attempts, 0,
            "keyboard-interactive should not run when it is not advertised"
        );
    }

    #[tokio::test]
    async fn connect_fails_when_publickey_is_rejected_outright() {
        let server = start_mfa_test_server(
            Vec::new(),
            PublicKeyOutcome::Rejected,
            KeyboardInteractiveOutcome::Accept,
        )
        .await;

        let result = authenticate_against_test_server(&server, ed25519_test_key()).await;
        let error = result.expect_err("authentication should fail when the key is rejected");
        let message = error.to_string();

        assert!(
            message.contains("Public key authentication failed"),
            "error should explain the key was rejected: {message}"
        );
        assert!(
            message.contains("partial success: false"),
            "error should report that no factor was accepted: {message}"
        );

        let log = auth_log(&server);
        assert_eq!(
            log.keyboard_interactive_attempts, 0,
            "keyboard-interactive should not run when the key is rejected"
        );
    }

    #[tokio::test]
    async fn connect_stops_after_keyboard_interactive_round_cap() {
        let server = start_mfa_test_server(
            vec!["Verification code: ".to_string()],
            PublicKeyOutcome::PartialKeyboardInteractive,
            KeyboardInteractiveOutcome::RepeatPrompts,
        )
        .await;

        let result = authenticate_against_test_server(&server, ed25519_test_key()).await;
        let error = result.expect_err("authentication should fail after the round cap");
        let message = error.to_string();

        assert!(
            message.contains(&format!(
                "did not finish after {MAX_KEYBOARD_INTERACTIVE_ROUNDS} rounds"
            )),
            "error should name the round cap: {message}"
        );

        let log = auth_log(&server);
        assert_eq!(
            log.keyboard_interactive_attempts,
            MAX_KEYBOARD_INTERACTIVE_ROUNDS + 1,
            "one start plus exactly MAX_KEYBOARD_INTERACTIVE_ROUNDS answered rounds"
        );

        // The count the error reports must be the number of response rounds the
        // server actually observed. The start request is not a round.
        let reported_rounds: usize = message
            .split("did not finish after ")
            .nth(1)
            .and_then(|rest| rest.split(' ').next())
            .and_then(|count| count.parse().ok())
            .expect("error should report the number of attempted rounds");
        assert_eq!(
            log.keyboard_interactive_attempts - 1,
            reported_rounds,
            "the reported round count must match the answered rounds"
        );
    }

    /// The second factor was refused: the public key was accepted as the first
    /// factor, but the keyboard-interactive answers were rejected. This must
    /// surface as an authentication failure that names the phase, never as a
    /// silent success.
    #[tokio::test]
    async fn connect_fails_when_keyboard_interactive_is_rejected() {
        let server = start_mfa_test_server(
            vec!["Verification code: ".to_string()],
            PublicKeyOutcome::PartialKeyboardInteractive,
            KeyboardInteractiveOutcome::Reject,
        )
        .await;

        let result = authenticate_against_test_server(&server, ed25519_test_key()).await;
        let error =
            result.expect_err("authentication should fail when the second factor is rejected");
        assert!(
            matches!(&error, SshError::AuthenticationFailed(_)),
            "the round should report an authentication failure: {error:?}"
        );

        let message = error.to_string();
        assert!(
            message.contains("keyboard-interactive rejected"),
            "error should name the failing phase: {message}"
        );
        assert!(
            message.contains("partial success: false"),
            "error should report that the round did not complete: {message}"
        );
        assert!(
            message.contains("KeyboardInteractive"),
            "error should name the remaining method: {message}"
        );

        let log = auth_log(&server);
        assert_eq!(
            log.keyboard_interactive_attempts, 2,
            "one round to receive the prompt, one to have the answers rejected"
        );
    }

    #[tokio::test]
    async fn connect_completes_keyboard_interactive_for_rsa_key() {
        let server = start_mfa_test_server(
            Vec::new(),
            PublicKeyOutcome::PartialKeyboardInteractive,
            KeyboardInteractiveOutcome::Accept,
        )
        .await;

        let result = authenticate_against_test_server(&server, rsa_test_key()).await;
        assert!(
            result.is_ok(),
            "an RSA key should complete the keyboard-interactive round: {result:?}"
        );

        let log = auth_log(&server);
        assert_eq!(
            log.publickey_attempts, 1,
            "the hash-algorithm fallback loop must not retry after a partial success"
        );
        assert_eq!(log.keyboard_interactive_attempts, 1);
    }
}
