"""Tests for keyserver.py SSH/Kerberos connection logic."""

import os
import sys
import unittest
from unittest.mock import MagicMock, patch

# Add keyserver directory to path so we can import the module
sys.path.insert(0, os.path.join(os.path.dirname(__file__), os.pardir))

import keyserver


class TestGetSshConnection(unittest.TestCase):
    """Tests for the get_ssh_connection function."""

    @patch("keyserver.subprocess.check_call")
    @patch("keyserver.paramiko.SSHClient")
    @patch("keyserver.paramiko.RSAKey.from_private_key")
    def test_ssh_key_connection(self, mock_from_private_key, mock_ssh_class, mock_check_call):
        """Test that SSH key connection uses RSA key and does not call kinit."""
        mock_ssh = MagicMock()
        mock_transport = MagicMock()
        mock_channel = MagicMock()
        mock_transport.open_channel.return_value = mock_channel
        mock_ssh.get_transport.return_value = mock_transport
        mock_ssh_class.return_value = mock_ssh

        mock_pkey = MagicMock()
        mock_from_private_key.return_value = mock_pkey

        client, channel = keyserver.get_ssh_connection(
            "host.example.com", "user1", "rsa_key_data"
        )

        # Should NOT call kinit
        mock_check_call.assert_not_called()

        # Should load RSA key from private key string
        mock_from_private_key.assert_called_once()

        # Should connect with pkey, not gss_auth
        mock_ssh.connect.assert_called_once_with(
            "host.example.com", username="user1", pkey=mock_pkey
        )

        # Should return the client and channel
        self.assertEqual(client, mock_ssh)
        self.assertEqual(channel, mock_channel)

    @patch("keyserver.subprocess.check_call")
    @patch("keyserver.paramiko.SSHClient")
    def test_kerberos_connection(self, mock_ssh_class, mock_check_call):
        """Test that Kerberos connection calls kinit and uses gss_auth."""
        mock_ssh = MagicMock()
        mock_transport = MagicMock()
        mock_channel = MagicMock()
        mock_transport.open_channel.return_value = mock_channel
        mock_ssh.get_transport.return_value = mock_transport
        mock_ssh_class.return_value = mock_ssh

        client, channel = keyserver.get_ssh_connection(
            "krb.example.com", "krbuser", "",
            keytab="/etc/krb5.keytab", principal="krbuser@EXAMPLE.COM"
        )

        # Should call kinit with the keytab and principal
        mock_check_call.assert_called_once_with(
            ["kinit", "-kt", "/etc/krb5.keytab", "krbuser@EXAMPLE.COM"]
        )

        # Should connect with gss_auth and gss_kex
        mock_ssh.connect.assert_called_once_with(
            "krb.example.com", username="krbuser", gss_auth=True, gss_kex=True
        )

    @patch("keyserver.subprocess.check_call")
    @patch("keyserver.paramiko.SSHClient")
    @patch("keyserver.paramiko.RSAKey.from_private_key")
    def test_kerberos_not_used_when_keytab_empty(self, mock_from_private_key, mock_ssh_class, mock_check_call):
        """Test that empty keytab/principal falls through to RSA key path."""
        mock_ssh = MagicMock()
        mock_transport = MagicMock()
        mock_channel = MagicMock()
        mock_transport.open_channel.return_value = mock_channel
        mock_ssh.get_transport.return_value = mock_transport
        mock_ssh_class.return_value = mock_ssh

        mock_pkey = MagicMock()
        mock_from_private_key.return_value = mock_pkey

        keyserver.get_ssh_connection(
            "host.example.com", "user1", "rsa_key",
            keytab="", principal=""
        )

        # Should NOT call kinit
        mock_check_call.assert_not_called()

        # Should connect with pkey
        mock_ssh.connect.assert_called_once_with(
            "host.example.com", username="user1", pkey=mock_pkey
        )

    @patch("keyserver.subprocess.check_call")
    @patch("keyserver.paramiko.SSHClient")
    @patch("keyserver.paramiko.RSAKey.from_private_key")
    def test_kerberos_not_used_when_only_keytab_set(self, mock_from_private_key, mock_ssh_class, mock_check_call):
        """Test that having keytab but no principal falls through to RSA key path."""
        mock_ssh = MagicMock()
        mock_transport = MagicMock()
        mock_channel = MagicMock()
        mock_transport.open_channel.return_value = mock_channel
        mock_ssh.get_transport.return_value = mock_transport
        mock_ssh_class.return_value = mock_ssh

        mock_pkey = MagicMock()
        mock_from_private_key.return_value = mock_pkey

        keyserver.get_ssh_connection(
            "host.example.com", "user1", "rsa_key",
            keytab="/etc/krb5.keytab", principal=""
        )

        # Should NOT call kinit (need both keytab and principal)
        mock_check_call.assert_not_called()

    @patch("keyserver.subprocess.check_call")
    @patch("keyserver.paramiko.SSHClient")
    @patch("keyserver.paramiko.RSAKey.from_private_key")
    def test_kerberos_not_used_when_only_principal_set(self, mock_from_private_key, mock_ssh_class, mock_check_call):
        """Test that having principal but no keytab falls through to RSA key path."""
        mock_ssh = MagicMock()
        mock_transport = MagicMock()
        mock_channel = MagicMock()
        mock_transport.open_channel.return_value = mock_channel
        mock_ssh.get_transport.return_value = mock_transport
        mock_ssh_class.return_value = mock_ssh

        mock_pkey = MagicMock()
        mock_from_private_key.return_value = mock_pkey

        keyserver.get_ssh_connection(
            "host.example.com", "user1", "rsa_key",
            keytab="", principal="user@REALM"
        )

        # Should NOT call kinit
        mock_check_call.assert_not_called()


class TestTryConnect(unittest.TestCase):
    """Tests for the try_connect function."""

    @patch("keyserver.subprocess.check_output")
    def test_localhost_uses_subprocess(self, mock_check_output):
        """Test that localhost connections use subprocess instead of SSH."""
        keyserver.ssh_host_name = "localhost"
        keyserver.ssh_path = "/test/path"
        keyserver.ssh_token = "test_token_123"

        keyserver.try_connect()

        # Verify subprocess was called with the correct command containing the token
        mock_check_output.assert_called_once()
        cmd = mock_check_output.call_args[0][0]
        self.assertIn("test_token_123", cmd)
        self.assertIn("/test/path", cmd)
        self.assertIn("adacs_job_client", cmd)

    @patch("keyserver.paramiko.RSAKey.from_private_key")
    @patch("keyserver.paramiko.SSHClient")
    def test_remote_host_uses_ssh(self, mock_ssh_class, mock_from_private_key):
        """Test that remote host connections use Paramiko SSH with RSA key."""
        keyserver.ssh_host_name = "remote.example.com"
        keyserver.ssh_user_name = "remoteuser"
        keyserver.ssh_path = "/remote/path"
        keyserver.ssh_key = "fake_rsa_key"
        keyserver.ssh_token = "remote_token_456"
        keyserver.ssh_keytab = ""
        keyserver.ssh_principal = ""

        mock_ssh = MagicMock()
        mock_transport = MagicMock()
        mock_channel = MagicMock()
        mock_channel.recv_ready.return_value = False
        mock_channel.recv_stderr_ready.return_value = False
        mock_channel.exit_status_ready.return_value = True
        mock_transport.open_channel.return_value = mock_channel
        mock_ssh.get_transport.return_value = mock_transport
        mock_ssh_class.return_value = mock_ssh

        mock_pkey = MagicMock()
        mock_from_private_key.return_value = mock_pkey

        keyserver.try_connect()

        # Verify SSH connect was called with pkey
        mock_ssh.connect.assert_called_once_with(
            "remote.example.com", username="remoteuser", pkey=mock_pkey
        )

        # Verify exec_command was called with a command containing the token
        mock_channel.exec_command.assert_called_once()
        cmd = mock_channel.exec_command.call_args[0][0]
        self.assertIn("remote_token_456", cmd)
        self.assertIn("/remote/path", cmd)

        # Verify connections were closed
        mock_channel.close.assert_called_once()
        mock_ssh.close.assert_called_once()

    @patch("keyserver.get_ssh_connection")
    def test_remote_host_with_kerberos(self, mock_get_ssh):
        """Test that remote connections pass keytab/principal to get_ssh_connection."""
        keyserver.ssh_host_name = "krb.example.com"
        keyserver.ssh_user_name = "krbuser"
        keyserver.ssh_path = "/krb/path"
        keyserver.ssh_key = ""
        keyserver.ssh_token = "krb_token_789"
        keyserver.ssh_keytab = "/etc/krb5.keytab"
        keyserver.ssh_principal = "krbuser@EXAMPLE.COM"

        mock_ssh = MagicMock()
        mock_channel = MagicMock()
        mock_channel.recv_ready.return_value = False
        mock_channel.recv_stderr_ready.return_value = False
        mock_channel.exit_status_ready.return_value = True
        mock_get_ssh.return_value = (mock_ssh, mock_channel)

        keyserver.try_connect()

        # Verify get_ssh_connection was called with keytab and principal
        mock_get_ssh.assert_called_once_with(
            "krb.example.com", "krbuser", "",
            keytab="/etc/krb5.keytab", principal="krbuser@EXAMPLE.COM"
        )


class TestMain(unittest.TestCase):
    """Tests for the main() entry point."""

    @patch("keyserver.try_connect")
    @patch("builtins.exit")
    def test_successful_connection_exits_zero(self, mock_exit, mock_try_connect):
        """Test that a successful connection results in exit(0)."""
        keyserver.main()

        mock_try_connect.assert_called_once()
        mock_exit.assert_called_once_with(0)

    @patch("keyserver.try_connect", side_effect=Exception("Connection failed"))
    @patch("builtins.exit")
    def test_failed_connection_exits_one(self, mock_exit, mock_try_connect):
        """Test that a failed connection results in exit(1)."""
        keyserver.main()

        mock_try_connect.assert_called_once()
        mock_exit.assert_called_once_with(1)


if __name__ == "__main__":
    unittest.main()
