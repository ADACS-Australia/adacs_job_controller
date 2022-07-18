venv/bin/python manage.py makemigrations
venv/bin/python manage.py migrate

mysqldump -d -u jobserver -pjobserver jobserver > schema.ddl

venv/bin/python  ../../Lib/sqlpp11/scripts/ddl2cpp schema.ddl jobserver_schema schema

echo -e "// NOLINTBEGIN\n$(cat jobserver_schema.h)\n// NOLINTEND" > jobserver_schema.h
mv jobserver_schema.h ../../Lib/
