# Generated by Django 3.2.2 on 2021-07-05 06:11

from django.db import migrations, models
import django.db.models.deletion


class Migration(migrations.Migration):

    dependencies = [
        ('jobserver', '0016_migrate_bilby_jobs'),
    ]

    operations = [
        migrations.CreateModel(
            name='FileListCache',
            fields=[
                ('id', models.AutoField(auto_created=True, primary_key=True, serialize=False, verbose_name='ID')),
                ('timestamp', models.DateTimeField(db_index=True)),
                ('path', models.TextField()),
                ('is_dir', models.BooleanField(default=False)),
                ('fileSize', models.BigIntegerField()),
                ('permissions', models.IntegerField()),
                ('job', models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, to='jobserver.job')),
            ],
        ),
    ]
