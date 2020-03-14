import sys
import os.path
import json
import traceback
import subprocess
import paramiko
import io
import time

cluster = sys.argv[1]
token = sys.argv[2]
current_directory = os.path.dirname(os.path.realpath(__file__))


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


def get_cluster_config():
    """
    Returns the json configuration dictionary for the cluster specified as the first parameter
    :return: A json object representing the specified cluster details
    """
    # Open the cluster json config
    with open(os.path.join(current_directory, '..', 'cluster_config.json')) as f:
        cluster_config = json.load(f)

    # Get the config for the specified cluster
    for c in cluster_config:
        if c['name'] == cluster:
            return c

    return None


def get_key(key_file):
    """
    Loads the specified key files and returns the key as text
    :param key_file: The filename of the key
    :return: The key's text content
    """
    # Open the key file
    with open(os.path.join(current_directory, 'keys', key_file)) as f:
        # Read all the content and return it
        return f.read()


def try_connect():
    """
    Attempts to connect to the specified cluster
    :return: Nothing
    """
    # Get the cluster json configuration dictionary
    cluster_json = get_cluster_config()

    # Read the relevant data
    host_name = cluster_json['host_name']
    client_path = cluster_json['client_path']
    key_file = cluster_json['key']
    user_name = cluster_json['user_name']

    # Check if the host is localhost
    if host_name == 'localhost':
        # Use subprocess to start the client locally
        subprocess.check_output(
            "cd {}; . venv/bin/activate; python client.py {}".format(client_path, token),
            shell=True
        )
    else:
        # Try to load the key
        key = get_key(key_file)

        # Try to create the ssh connection
        client, ssh = get_ssh_connection(host_name, user_name, key)

        # Construct the command
        command = "cd {}; . venv/bin/activate; python client.py {}".format(client_path, token)

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
