# Generated by Django 3.0.4 on 2020-05-17 22:33

from django.db import migrations, models


class Migration(migrations.Migration):

    dependencies = [
        ('jobserver', '0010_jobhistory_what'),
    ]

    operations = [
        migrations.CreateModel(
            name='ClusterUuid',
            fields=[
                ('id', models.AutoField(auto_created=True, primary_key=True, serialize=False, verbose_name='ID')),
                ('cluster', models.CharField(db_index=True, max_length=200)),
                ('uuid', models.CharField(db_index=True, max_length=36, unique=True)),
                ('timestamp', models.DateTimeField(db_index=True)),
            ],
        ),
    ]