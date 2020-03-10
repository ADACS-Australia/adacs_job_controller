from django.db import models


class Job(models.Model):
    # The id of the user for this job
    user = models.IntegerField()

    # The parameters for this job (Use base64 if you need to store binary)
    parameters = models.TextField()


class JobHistory(models.Model):
    # The job this history object belongs to
    job = models.ForeignKey(Job, on_delete=models.CASCADE)

    # When this update occurred
    timestamp = models.DateTimeField()

    # The state for the update
    state = models.IntegerField()

    # Any additional details
    details = models.TextField()


class FileDownload(models.Model):
    # The if of the user for this job
    user = models.IntegerField()

    # The UUID of this file download
    uuid = models.CharField(max_length=36)

    # When the file download was created
    timestamp = models.DateTimeField()

