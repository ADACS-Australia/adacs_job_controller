# Generated by Django 4.1 on 2022-10-20 07:45

from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ('jobserver', '0022_bundlejob_clusterjob_clusterjobstatus'),
    ]

    operations = [
        migrations.AddField(
            model_name='clusterjob',
            name='deleted',
            field=models.BooleanField(default=False),
        ),
        migrations.AddField(
            model_name='clusterjob',
            name='deleting',
            field=models.BooleanField(default=False),
        ),
    ]
