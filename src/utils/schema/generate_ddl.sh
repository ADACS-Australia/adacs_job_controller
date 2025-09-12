venv/bin/python manage.py makemigrations
venv/bin/python manage.py migrate

mysqldump -d -u jobserver -pjobserver jobserver > schema.ddl

venv/bin/python  ../../third_party/sqlpp11/scripts/ddl2cpp schema.ddl jobserver_schema schema

echo "// NOLINTBEGIN" > temp_schema.h
cat jobserver_schema.h >> temp_schema.h
echo "// NOLINTEND" >> temp_schema.h
mv temp_schema.h jobserver_schema.h

# Convert header to C++20 module
venv/bin/python convert_schema_to_module.py jobserver_schema.h jobserver_schema.ixx

# Move both files to Lib directory
mv jobserver_schema.h ../../Lib/
mv jobserver_schema.ixx ../../Lib/

# Remove the old header file since we now have the module
rm ../../Lib/jobserver_schema.h
