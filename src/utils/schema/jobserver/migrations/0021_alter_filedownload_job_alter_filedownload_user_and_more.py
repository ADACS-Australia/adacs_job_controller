# Generated by Django 4.1 on 2022-08-12 04:57

from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ('jobserver', '0020_alter_clusteruuid_id_alter_filedownload_id_and_more'),
    ]

    operations = [
        migrations.AlterField(
            model_name='filedownload',
            name='job',
            field=models.BigIntegerField(),
        ),
        migrations.AlterField(
            model_name='filedownload',
            name='user',
            field=models.BigIntegerField(),
        ),
        migrations.AlterField(
            model_name='job',
            name='user',
            field=models.BigIntegerField(),
        ),
    ]
