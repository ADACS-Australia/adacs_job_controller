# Generated by Django 3.0.4 on 2020-03-09 21:40

from django.db import migrations, models
import django.db.models.deletion


class Migration(migrations.Migration):

    dependencies = [
        ('jobserver', '0001_initial'),
    ]

    operations = [
        migrations.CreateModel(
            name='FileDownload',
            fields=[
                ('id', models.AutoField(auto_created=True, primary_key=True, serialize=False, verbose_name='ID')),
                ('user', models.IntegerField()),
                ('uuid', models.CharField(max_length=36)),
                ('timestamp', models.DateTimeField()),
            ],
        ),
        migrations.RemoveField(
            model_name='job',
            name='state',
        ),
        migrations.AddField(
            model_name='job',
            name='user',
            field=models.IntegerField(default=0),
            preserve_default=False,
        ),
        migrations.CreateModel(
            name='JobHistory',
            fields=[
                ('id', models.AutoField(auto_created=True, primary_key=True, serialize=False, verbose_name='ID')),
                ('timestamp', models.DateTimeField()),
                ('state', models.IntegerField()),
                ('details', models.TextField()),
                ('job', models.ForeignKey(on_delete=django.db.models.deletion.CASCADE, to='jobserver.Job')),
            ],
        ),
    ]
