# GWCloud Job Server

The GWCloud Job Server is a C++ project that manages the server side of the Job Controller Server/Client architecture. 



## Software Architecture

The job server has three distinct components:-

* The C++ server code, which is a CMake project
* The Schema/DDL python project - for maintaining database schema and migration state used by the C++ component.
* The keyserver python project - called by the C++ component for starting remote clients via SSH.



## Local Development

### Prerequisites

Several system libraries are required for local development. Package names for Ubuntu 22.04 can be found in `docker/gwcloud_job_server.Dockerfile`, but at the time of writing, that list looks like:

```
python3 python3-venv gcovr mariadb-client libunwind-dev libdw-dev libgtest-dev libmysqlclient-dev build-essential cmake libboost-dev libgoogle-glog-dev libboost-test-dev libboost-system-dev libboost-thread-dev libboost-coroutine-dev libboost-context-dev libssl-dev libboost-filesystem-dev libboost-program-options-dev libboost-regex-dev libevent-dev libfmt-dev libdouble-conversion-dev libcurl4-openssl-dev git libjemalloc-dev libzstd-dev liblz4-dev libsnappy-dev libbz2-dev valgrind libdwarf-dev libfast-float-dev clang-tidy ninja-build libcpp-jwt-dev libhowardhinnant-date-dev nlohmann-json3-dev
```

**Note**: The project now uses C++20 modules and requires the `ninja-build` package for optimal build performance.

### Initial Setup

If this is a freshly checked out repository, you'll need to initialize and update the Git submodules:

```bash
git submodule update --init --recursive
```

This will pull in the required third-party dependencies:
- Simple-Web-Server
- Simple-WebSocket-Server  
- folly
- sqlpp11



This project makes heavy use of docker for testing and building the project in a controlled environment. You will need mysql running on the local host if you wish to run the tests locally, with a user configured. See `Settings.ixx` for the expected user details - they can be overridden using environment variables.



## Building

The project uses C++20 modules and requires the Ninja build system. The main targets are `adacs_job_controller` (runtime binary) and `Boost_Tests_run` (test suite).

### Standard Build Process

```bash
cd src
mkdir build
cd build
cmake -G Ninja -DCMAKE_BUILD_TYPE=Debug ..
ninja
```

### Build Options

**Static Analysis**: By default, clang-tidy static analysis is enabled. To disable it for faster builds:

```bash
cmake -G Ninja -DCMAKE_BUILD_TYPE=Debug -DENABLE_CLANG_TIDY=OFF ..
```

### Running Tests

You can run the generated `Boost_Tests_run` target which will execute the full test suite:

```bash
./Boost_Tests_run
```

A comprehensive test suite (used by the CI) can be run by running `bash scripts/test.sh` from the repository root. This will report any test failures, and will also generate a code coverage report. To run valgrind on the project, another script exists `bash scripts/valgrind.sh` - you can expect this to take some time to run.

Finally, to build the production docker asset, run `bash scripts/build.sh`, then push the docker image.



## Deployment Configuration

The Job Controller has two main configuration environment variables that are read at startup. These are `CLUSTER_CONFIG` and `ACCESS_SECRET_CONFIG`. Both of these environment variable are **base64** encoded JSON objects.



The `CLUSTER_CONFIG` configures the remote clients and their SSH connection details, while `ACCESS_SECRET_CONFIG` configures the HTTP access and restricts applications to specific cluster(s). In this context an application is a JWT token with associated access to specific clusters.



In kubernetes where the job controller is deployed, we use a vault service to store these environment variables as secrets. They can be seen here: https://vault.gwdc.org.au/ui/vault/secrets/gwcloud%2Fkv/show/job-server



The typical process to update these environment variables is to copy the existing value from vault, decode it with https://www.base64decode.org/, copy the decoded JSON object in to https://jsoneditoronline.org/, make the required changes to the JSON, copy the new JSON in to https://www.base64encode.org/ and copy the base64 encoded value back to vault and create a new secrets version.



The format for `CLUSTER_CONFIG` is:

```
[
  {
    "name": "ozstar", 							# The cluster name - must be unique,
    "host": "ozstar.swin.edu.au",				# The SSH host name
    "username": "bilby",						# The SSH username
    "path": "/fred/oz988/gwcloud_job_client/",	# The remote path to the job controller client
    "key": "-----BEGIN RSA PRIVATE KEY-----..."	# The SSH *RSA* private key used to connect
  },
  ...
]
```



The format for `ACCESS_SECRET_CONFIG` is:

```
[
  {
    "name": "bilbyui",							# The application name
    "secret": "super_secret",					# A very long and complex JWT secret key (ideally a 128 character string with symbols and numbers)
    "applications": [							# A list of other applications, if any, that this application can access (ie read job information)
      "gwlab"
    ],
    "clusters": [								# A list of clusters that this application can access (ie to submit jobs to)
      "ozstar",
      "cit"
    ]
  },
  ...
]
```



The typical process to add a new application would look something like the following:

1. Create or gain access to the remote SSH user who will be running the job controller client. Typically this user should be a system user.
2. Install and configure the job controller client on that remote machine. (Refer to https://github.com/gravitationalwavedc/gwcloud_job_client)
3. Create a new **RSA** ssh key pair and add the public key to the remote SSH user (Note: OPENSSH keys won't work) (Add option `-m PEM` into your ssh-keygen command. For example, you can run `ssh-keygen -m PEM -t rsa -b 4096 -C "your_email@example.com"` to force ssh-keygen to export as PEM format.)
4. Create a new entry in the `CLUSTER_CONFIG` config with the cluster name and SSH details
5. Create a new entry in the `ACCESS_SECRET_CONFIG` config with the application name, JWT secret, and the cluster name from step 4 in the `clusters` list.
6. Update the vault secret
7. Restart the job controller deployment in Argo CD https://cd.gwdc.org.au/applications/gwcloud-job-server to apply the new secrets.



## Using the API 

The job controller server exposes a RESTful API that uses JWT authentication. There are two main objects that can be operated on, jobs and files. The Job API is under the url path `/job/apiv1/job/`, and the File API is under `/job/apiv1/file/`. Most (but not all) API requests require a JWT `Authorization` header to be sent in the request, refer to https://jwt.io/introduction for more details.



### Job API

#### GET

Fetch the status of and/or filter for job(s). 

Request query parameters;

```
Query parameters (All optional)
  jobIds:          fetch array of jobs by ID (CSV separated)
  startTimeGt:     start time greater than this (Newer than this) (Integer epoch time)
  startTimeLt:     start time less than this (Older than this) (Integer epoch time)
  endTimeGt:       end time greater than this (Newer than this) (Integer epoch time)
  endTimeLt:       end time less than this (Older than this) (Integer epoch time)
  Job Step filtering (Must include a job step id and at least one filter parameter). Job step filtering filters by the provided job step's MOST RECENT state.
  jobSteps:        csv list of:-
    jobStepId:     the name of the job step id to filter on
    state:         the state of the job step

Job steps are combined using OR

So a job filter might look like

/job/apiv1/job/?jobIDs=50,51,52&startTimeLt=1589838778&endTimeGt=1589836778&jobSteps=jid0,500,jid1,500

Which will return any jobs with ID's 51, 51, or 52, with a start time less than 1589838778 and greater than 1589836778, which have job steps with jid0 = 500 or jid1 = 500.
```



The return JSON object;

```
[
  {
    "id": 5,					# Job ID
    "user": 32,					# Id of the user who submitted the job
    "parameters": "whatever",	# The parameter payload used to launch the job
    "cluster": "ozstar",		# The cluster the job was/will be submitted to
    "bundle": "whatever",		# The bundle hash of the bundle that has/will handle the job
    "history": [				# A list of Job History objects for this job
      {
        "jobId": 5, 			# ID of the Job this Job History object is for
        "timestamp": 34233		# The timestamp when this Job History object was created
        "what": "jid0"			# The job step this Job History is for. Can be anything - is usually defined by the bundle implementation. May be "system" or "_job_completion_" for the final job outcome.
        "state": 500			# The state for this 
      },
      ...
    ]
  },
  ...
]
```



#### POST

Create and submit a new job. If submission fails, a Bad Request response will be sent.

POST payload;

```
{
  "cluster": "ozstar",			# The name of the cluster to submit the job to (Must be defined in ACCESS_SECRET_CONFIG for the JWT secret making the request)
  "userId": 32,					# The ID of the user who submitted the job (This is not enforced and can be anything)
  "parameters": "whatever",		# The parameter payload sent to the bundle to submit the job.
  "bundle": "whatever",			# The SHA1 hash of the client bundle to handle the job
}
```



The return JSON object;

```
{
  "jobId": 56					# The ID of the submitted job
}
```



#### PATCH

Cancel a job. If cancellation fails, a Bad Request response will be sent.

POST payload;

```
{
  "jobId": 56					# The ID of the job to cancel
}
```



The return JSON object;

```
{
  "cancelled": 56					# The ID of the job that was cancelled
}
```



#### DELETE

Delete a job. If cancellation fails, a Bad Request response will be sent. A job must not be in a running state to be deleted.

POST payload;

```
{
  "jobId": 56						# The ID of the job to delete
}
```



The return JSON object;

```
{
  "cancelled": 56					# The ID of the job that was deleted
}
```



### File API

#### GET

Download a file. This request does not require JWT authorization - instead relying on the passed file download ID. If a file download can not be initiated due to an error or invalid file download id, a Bad Request response will be sent. If a client is not online, or fails to respond during the file download process, a Service Unavailable response will be sent.



Query parameters;

```
fileId				# The file download ID generated from a POST verb
forceDownload		# If the response should trigger an attachment download or not
```



The returned response will be a streaming file download with the following headers;

```
Content-Type: application/octet-stream
Content-Length: remote file size
if forceDownload == True:
  Content-Disposition: attachment; filename="remote file name"
else:
  Content-Disposition: filename="remote file name"
```



#### POST

Creates new file download ID(s). If an issue occurs, a Bad Request response will be sent. Download IDs are to be used with the GET verb to actually download the file. If no paths are provided an empty array of ID's is generated.

POST payload;

```
A. Only providing 1 path
{
  "jobId": 56,				# The job ID to generate the file download ID for
  "path": "whatever"		# Path relative to the root of the remote job to genarate a file download ID for
}

B. Providing a list of paths
{
  "jobId": 56,				# The job ID to generate the file download ID for
  "paths": [				# A list of paths to generate download IDs for
    "path1",
    "path2",
    ...
  ]
}
```



The return JSON object;

```
A. Only providing 1 path
{
  "fileId": "some uuid"		# The generated file download ID
}

B. Providing a list of paths
{
  "fileIds": [				# A list of generated file download ID's. These are guarenteed to be in the same order as the provided path list.
    "uuid1",	
    "uuid2"
  ]
}
```



#### PATCH

Get a remote file list for a job. If fetching the file list fails, a Bad Request response will be sent.



POST payload;

```
{
  "jobId": 56,				# The ID of the job to fetch the file list for
  "recursive": true,		# If the result should be a recursive file list, or a file list just at the provided path
  "path": "/my/path/"		# The path relative to the root of the job that the file list should be returned for.
}
```



The return JSON object:

```
{
  "files": [				# The list of files returned
  	{
  	  "path": "/file/path", # The path to the file,
  	  "isDir": false,		# If the file is a directory or not
  	  "fileSize": 345652,	# The file size in bytes
  	  "permissions": null	# The permissions mask of the file (currently not implemented)
  	}
  ]
}
```

