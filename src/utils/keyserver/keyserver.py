import sys
import os.path
import traceback
import subprocess
import paramiko
import io
import time

# Read environment variables passed down from the host
ssh_host_name = os.getenv("SSH_HOST")
ssh_user_name = os.getenv("SSH_USERNAME")
ssh_path = os.getenv("SSH_PATH")
ssh_key = os.getenv("SSH_KEY")
ssh_token = os.getenv("SSH_TOKEN")


def get_ssh_connection(host_name, user_name, key):
    """
    Returns a Paramiko SSH connection to the cluster
    :return: The SSH instance
    """
    # Create an SSH client
    ssh = paramiko.SSHClient()
    ssh.set_missing_host_key_policy(paramiko.AutoAddPolicy())

    # If this fails and raises an exception then the top level exception handler
    # will be triggered
    key = io.StringIO(key)
    key = paramiko.RSAKey.from_private_key(key)
    ssh.connect(host_name, username=user_name, pkey=key)
    return ssh, ssh.get_transport().open_channel("session")


def try_connect():
    """
    Attempts to connect to the specified cluster
    :return: Nothing
    """
    # Check if the host is localhost - there is no need to initiate SSH when running locally
    if ssh_host_name == 'localhost':
        # Use subprocess to start the client locally
        subprocess.check_output(
            "cd {}; . venv/bin/activate; python client.py {}".format(ssh_path, ssh_token),
            shell=True
        )
    else:
        # Try to create the ssh connection
        client, ssh = get_ssh_connection(ssh_host_name, ssh_user_name, ssh_key)

        # Construct the command
        command = "cd {}; source env.sh; source venv/bin/activate; python client.py {}".format(ssh_path, ssh_token)

        # Execute the remote command to start the daemon
        ssh.exec_command(command)

        # Wait for the connection to close
        stdout, stderr = b'', b''
        while True:  # monitoring process
            # Reading from output streams
            while ssh.recv_ready():
                stdout += ssh.recv(1000)
            while ssh.recv_stderr_ready():
                stderr += ssh.recv_stderr(1000)
            if ssh.exit_status_ready():  # If completed
                break
            time.sleep(0.1)

        # Close the conneciton
        ssh.close()
        client.close()

        print("SSH command {} returned:".format(command))
        print("Stdout: {}".format(stdout))
        print("Stderr: {}".format(stderr))


try:
    # Attempt to connect to the remote host
    try_connect()

    # Exit with success status
    exit(0)

except Exception as e:
    # An exception occurred, log the exception to the log
    print("Error initiating the remote client")
    print(type(e))
    print(e.args)
    print(e)

    # Also log the stack trace
    exc_type, exc_value, exc_traceback = sys.exc_info()
    lines = traceback.format_exception(exc_type, exc_value, exc_traceback)
    print(''.join('!! ' + line for line in lines))

    # Exit with failure status
    exit(1)
