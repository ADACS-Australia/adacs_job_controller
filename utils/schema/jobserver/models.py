from django.db import models

# Create your models here.


class Job(models.Model):
    parameters = models.TextField()
    state = models.IntegerField()