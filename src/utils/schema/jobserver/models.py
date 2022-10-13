from django.db import models


class Job(models.Model):
    # The id of the user for this job. This is set from the userId field of the JWT payload
    user = models.BigIntegerField()

    # The parameters for this job (Use base64 if you need to store binary)
    parameters = models.TextField()

    # The cluster the job ran on
    cluster = models.CharField(max_length=200)

    # The bundle for this job
    bundle = models.CharField(max_length=40)

    # The application this job is for. This is set from the name associated with the secret that created the job
    application = models.CharField(max_length=32)


class JobHistory(models.Model):
    # The job this history object belongs to
    job = models.ForeignKey(Job, on_delete=models.CASCADE, db_index=True)

    # When this update occurred
    timestamp = models.DateTimeField(db_index=True)

    # What this update was for. Usually a job step or system
    what = models.CharField(max_length=128, db_index=True)

    # The state for the update
    state = models.IntegerField(db_index=True)

    # Any additional details
    details = models.TextField()


class FileDownload(models.Model):
    # The id of the user for this job
    user = models.BigIntegerField()

    # The job ID this download is for
    job = models.BigIntegerField()

    # The cluster name this job belongs to
    cluster = models.CharField(max_length=200)

    # The bundle that this job belongs to
    bundle = models.CharField(max_length=40)

    # The UUID of this file download
    uuid = models.CharField(max_length=36, db_index=True, unique=True)

    # The path to the file to download (Relative to the job working directory)
    path = models.TextField()

    # When the file download was created
    timestamp = models.DateTimeField(db_index=True)


class ClusterUuid(models.Model):
    # The cluster this uuid is for
    cluster = models.CharField(max_length=200)

    # The UUID
    uuid = models.CharField(max_length=36, db_index=True, unique=True)

    # The timestamp when this uuid was created
    timestamp = models.DateTimeField(db_index=True)


class FileListCache(models.Model):
    class Meta:
        unique_together = ('job', 'path')

    # The job this file list cache record is for
    job = models.ForeignKey(Job, on_delete=models.CASCADE, db_index=True)

    # The timestamp when this file list cache record was created
    timestamp = models.DateTimeField(db_index=True)

    # The file path
    path = models.CharField(max_length=765, db_index=True)

    # If the file path is a directory
    is_dir = models.BooleanField(default=False)

    # The size of the file
    file_size = models.BigIntegerField()

    # The file permissions
    permissions = models.IntegerField()


class ClusterJob(models.Model):
    # The cluster this job is for
    cluster = models.CharField(max_length=200, db_index=True)

    # The job controller job ID
    job_id = models.IntegerField(blank=True, null=True, default=None, db_index=True)

    # The scheduler/bundle ID
    scheduler_id = models.IntegerField(blank=True, null=True, default=None, db_index=True)

    # If this job is waiting on a submit bundle function (Controller won't try to check job status or resubmit while
    # this flag is true)
    submitting = models.BooleanField(default=False)
    submitting_count = models.IntegerField(default=0)

    # The bundle hash of this job
    bundle_hash = models.CharField(max_length=40)

    # The working directory of this job
    working_directory = models.CharField(max_length=512)

    # If the job is currently running
    running = models.BooleanField(default=True)


class ClusterJobStatus(models.Model):
    # Figure out the cluster via the job

    # The job this status object is for
    job = models.ForeignKey(ClusterJob, on_delete=models.CASCADE, related_name='status')

    # What this update was for, usually a job step id or system
    what = models.CharField(max_length=128)

    # The state for the update
    state = models.IntegerField(db_index=True)


class BundleJob(models.Model):
    # The cluster this job is for
    cluster = models.CharField(max_length=200, db_index=True)

    # The bundle hash for this bundle job
    bundle_hash = models.CharField(max_length=40, db_index=True)

    # The content for the job details (Usually a json dump)
    content = models.TextField()
