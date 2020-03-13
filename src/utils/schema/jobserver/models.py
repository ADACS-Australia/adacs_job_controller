from django.db import models


class Job(models.Model):
    # The id of the user for this job
    user = models.IntegerField()

    # The parameters for this job (Use base64 if you need to store binary)
    parameters = models.TextField()

    # The cluster the job ran on
    cluster = models.CharField(max_length=200)

    # The bundle for this job
    bundle = models.CharField(max_length=128)


class JobHistory(models.Model):
    # The job this history object belongs to
    job = models.ForeignKey(Job, on_delete=models.CASCADE, db_index=True)

    # When this update occurred
    timestamp = models.DateTimeField(db_index=True)

    # The state for the update
    state = models.IntegerField(db_index=True)

    # Any additional details
    details = models.TextField()


class FileDownload(models.Model):
    # The if of the user for this job
    user = models.IntegerField()

    # The job this download is for
    job = models.ForeignKey(Job, on_delete=models.CASCADE)

    # The UUID of this file download
    uuid = models.CharField(max_length=36, db_index=True, unique=True)

    # The path to the file to download (Relative to the job working directory)
    path = models.TextField()

    # When the file download was created
    timestamp = models.DateTimeField(db_index=True)
