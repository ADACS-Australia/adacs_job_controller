venv/bin/python manage.py makemigrations
venv/bin/python manage.py migrate

mysqldump -d -u jobserver -pjobserver jobserver > schema.ddl

venv/bin/python ddl2cpp.py schema.ddl jobserver_schema schema

cp jobserver_schema.h ../../Lib/