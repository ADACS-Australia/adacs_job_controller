# Generated by Django 3.0.4 on 2020-03-11 22:54

from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ('jobserver', '0006_auto_20200311_2252'),
    ]

    operations = [
        migrations.AlterField(
            model_name='filedownload',
            name='timestamp',
            field=models.DateTimeField(db_index=True),
        ),
        migrations.AlterField(
            model_name='filedownload',
            name='uuid',
            field=models.CharField(db_index=True, max_length=36),
        ),
        migrations.AlterField(
            model_name='jobhistory',
            name='state',
            field=models.IntegerField(db_index=True),
        ),
        migrations.AlterField(
            model_name='jobhistory',
            name='timestamp',
            field=models.DateTimeField(db_index=True),
        ),
    ]
