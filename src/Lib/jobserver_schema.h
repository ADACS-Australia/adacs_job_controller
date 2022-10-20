// NOLINTBEGIN
// generated by ../../third_party/sqlpp11/scripts/ddl2cpp schema.ddl jobserver_schema schema
#ifndef SCHEMA_JOBSERVER_SCHEMA_H
#define SCHEMA_JOBSERVER_SCHEMA_H

#include <sqlpp11/table.h>
#include <sqlpp11/data_types.h>
#include <sqlpp11/char_sequence.h>

namespace schema
{
  namespace DjangoMigrations_
  {
    struct Id
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "id";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T id;
            T& operator()() { return id; }
            const T& operator()() const { return id; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::bigint, sqlpp::tag::must_not_insert, sqlpp::tag::must_not_update>;
    };
    struct App
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "app";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T app;
            T& operator()() { return app; }
            const T& operator()() const { return app; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::varchar, sqlpp::tag::require_insert>;
    };
    struct Name
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "name";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T name;
            T& operator()() { return name; }
            const T& operator()() const { return name; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::varchar, sqlpp::tag::require_insert>;
    };
    struct Applied
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "applied";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T applied;
            T& operator()() { return applied; }
            const T& operator()() const { return applied; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::time_point, sqlpp::tag::require_insert>;
    };
  } // namespace DjangoMigrations_

  struct DjangoMigrations: sqlpp::table_t<DjangoMigrations,
               DjangoMigrations_::Id,
               DjangoMigrations_::App,
               DjangoMigrations_::Name,
               DjangoMigrations_::Applied>
  {
    struct _alias_t
    {
      static constexpr const char _literal[] =  "django_migrations";
      using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
      template<typename T>
      struct _member_t
      {
        T djangoMigrations;
        T& operator()() { return djangoMigrations; }
        const T& operator()() const { return djangoMigrations; }
      };
    };
  };
  namespace JobserverBundlejob_
  {
    struct Id
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "id";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T id;
            T& operator()() { return id; }
            const T& operator()() const { return id; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::bigint, sqlpp::tag::must_not_insert, sqlpp::tag::must_not_update>;
    };
    struct Cluster
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "cluster";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T cluster;
            T& operator()() { return cluster; }
            const T& operator()() const { return cluster; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::varchar, sqlpp::tag::require_insert>;
    };
    struct BundleHash
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "bundle_hash";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T bundleHash;
            T& operator()() { return bundleHash; }
            const T& operator()() const { return bundleHash; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::varchar, sqlpp::tag::require_insert>;
    };
    struct Content
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "content";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T content;
            T& operator()() { return content; }
            const T& operator()() const { return content; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::text, sqlpp::tag::require_insert>;
    };
  } // namespace JobserverBundlejob_

  struct JobserverBundlejob: sqlpp::table_t<JobserverBundlejob,
               JobserverBundlejob_::Id,
               JobserverBundlejob_::Cluster,
               JobserverBundlejob_::BundleHash,
               JobserverBundlejob_::Content>
  {
    struct _alias_t
    {
      static constexpr const char _literal[] =  "jobserver_bundlejob";
      using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
      template<typename T>
      struct _member_t
      {
        T jobserverBundlejob;
        T& operator()() { return jobserverBundlejob; }
        const T& operator()() const { return jobserverBundlejob; }
      };
    };
  };
  namespace JobserverClusterjob_
  {
    struct Id
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "id";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T id;
            T& operator()() { return id; }
            const T& operator()() const { return id; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::bigint, sqlpp::tag::must_not_insert, sqlpp::tag::must_not_update>;
    };
    struct Cluster
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "cluster";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T cluster;
            T& operator()() { return cluster; }
            const T& operator()() const { return cluster; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::varchar, sqlpp::tag::require_insert>;
    };
    struct JobId
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "job_id";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T jobId;
            T& operator()() { return jobId; }
            const T& operator()() const { return jobId; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::integer, sqlpp::tag::can_be_null>;
    };
    struct SchedulerId
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "scheduler_id";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T schedulerId;
            T& operator()() { return schedulerId; }
            const T& operator()() const { return schedulerId; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::integer, sqlpp::tag::can_be_null>;
    };
    struct Submitting
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "submitting";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T submitting;
            T& operator()() { return submitting; }
            const T& operator()() const { return submitting; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::tinyint, sqlpp::tag::require_insert>;
    };
    struct SubmittingCount
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "submitting_count";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T submittingCount;
            T& operator()() { return submittingCount; }
            const T& operator()() const { return submittingCount; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::integer, sqlpp::tag::require_insert>;
    };
    struct BundleHash
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "bundle_hash";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T bundleHash;
            T& operator()() { return bundleHash; }
            const T& operator()() const { return bundleHash; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::varchar, sqlpp::tag::require_insert>;
    };
    struct WorkingDirectory
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "working_directory";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T workingDirectory;
            T& operator()() { return workingDirectory; }
            const T& operator()() const { return workingDirectory; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::varchar, sqlpp::tag::require_insert>;
    };
    struct Running
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "running";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T running;
            T& operator()() { return running; }
            const T& operator()() const { return running; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::tinyint, sqlpp::tag::require_insert>;
    };
    struct Deleted
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "deleted";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T deleted;
            T& operator()() { return deleted; }
            const T& operator()() const { return deleted; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::tinyint, sqlpp::tag::require_insert>;
    };
    struct Deleting
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "deleting";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T deleting;
            T& operator()() { return deleting; }
            const T& operator()() const { return deleting; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::tinyint, sqlpp::tag::require_insert>;
    };
  } // namespace JobserverClusterjob_

  struct JobserverClusterjob: sqlpp::table_t<JobserverClusterjob,
               JobserverClusterjob_::Id,
               JobserverClusterjob_::Cluster,
               JobserverClusterjob_::JobId,
               JobserverClusterjob_::SchedulerId,
               JobserverClusterjob_::Submitting,
               JobserverClusterjob_::SubmittingCount,
               JobserverClusterjob_::BundleHash,
               JobserverClusterjob_::WorkingDirectory,
               JobserverClusterjob_::Running,
               JobserverClusterjob_::Deleted,
               JobserverClusterjob_::Deleting>
  {
    struct _alias_t
    {
      static constexpr const char _literal[] =  "jobserver_clusterjob";
      using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
      template<typename T>
      struct _member_t
      {
        T jobserverClusterjob;
        T& operator()() { return jobserverClusterjob; }
        const T& operator()() const { return jobserverClusterjob; }
      };
    };
  };
  namespace JobserverClusterjobstatus_
  {
    struct Id
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "id";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T id;
            T& operator()() { return id; }
            const T& operator()() const { return id; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::bigint, sqlpp::tag::must_not_insert, sqlpp::tag::must_not_update>;
    };
    struct What
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "what";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T what;
            T& operator()() { return what; }
            const T& operator()() const { return what; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::varchar, sqlpp::tag::require_insert>;
    };
    struct State
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "state";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T state;
            T& operator()() { return state; }
            const T& operator()() const { return state; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::integer, sqlpp::tag::require_insert>;
    };
    struct JobId
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "job_id";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T jobId;
            T& operator()() { return jobId; }
            const T& operator()() const { return jobId; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::bigint, sqlpp::tag::require_insert>;
    };
  } // namespace JobserverClusterjobstatus_

  struct JobserverClusterjobstatus: sqlpp::table_t<JobserverClusterjobstatus,
               JobserverClusterjobstatus_::Id,
               JobserverClusterjobstatus_::What,
               JobserverClusterjobstatus_::State,
               JobserverClusterjobstatus_::JobId>
  {
    struct _alias_t
    {
      static constexpr const char _literal[] =  "jobserver_clusterjobstatus";
      using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
      template<typename T>
      struct _member_t
      {
        T jobserverClusterjobstatus;
        T& operator()() { return jobserverClusterjobstatus; }
        const T& operator()() const { return jobserverClusterjobstatus; }
      };
    };
  };
  namespace JobserverClusteruuid_
  {
    struct Id
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "id";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T id;
            T& operator()() { return id; }
            const T& operator()() const { return id; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::bigint, sqlpp::tag::must_not_insert, sqlpp::tag::must_not_update>;
    };
    struct Cluster
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "cluster";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T cluster;
            T& operator()() { return cluster; }
            const T& operator()() const { return cluster; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::varchar, sqlpp::tag::require_insert>;
    };
    struct Uuid
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "uuid";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T uuid;
            T& operator()() { return uuid; }
            const T& operator()() const { return uuid; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::varchar, sqlpp::tag::require_insert>;
    };
    struct Timestamp
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "timestamp";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T timestamp;
            T& operator()() { return timestamp; }
            const T& operator()() const { return timestamp; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::time_point, sqlpp::tag::require_insert>;
    };
  } // namespace JobserverClusteruuid_

  struct JobserverClusteruuid: sqlpp::table_t<JobserverClusteruuid,
               JobserverClusteruuid_::Id,
               JobserverClusteruuid_::Cluster,
               JobserverClusteruuid_::Uuid,
               JobserverClusteruuid_::Timestamp>
  {
    struct _alias_t
    {
      static constexpr const char _literal[] =  "jobserver_clusteruuid";
      using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
      template<typename T>
      struct _member_t
      {
        T jobserverClusteruuid;
        T& operator()() { return jobserverClusteruuid; }
        const T& operator()() const { return jobserverClusteruuid; }
      };
    };
  };
  namespace JobserverFiledownload_
  {
    struct Id
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "id";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T id;
            T& operator()() { return id; }
            const T& operator()() const { return id; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::bigint, sqlpp::tag::must_not_insert, sqlpp::tag::must_not_update>;
    };
    struct User
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "user";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T user;
            T& operator()() { return user; }
            const T& operator()() const { return user; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::bigint, sqlpp::tag::require_insert>;
    };
    struct Uuid
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "uuid";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T uuid;
            T& operator()() { return uuid; }
            const T& operator()() const { return uuid; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::varchar, sqlpp::tag::require_insert>;
    };
    struct Timestamp
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "timestamp";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T timestamp;
            T& operator()() { return timestamp; }
            const T& operator()() const { return timestamp; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::time_point, sqlpp::tag::require_insert>;
    };
    struct Job
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "job";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T job;
            T& operator()() { return job; }
            const T& operator()() const { return job; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::bigint, sqlpp::tag::require_insert>;
    };
    struct Path
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "path";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T path;
            T& operator()() { return path; }
            const T& operator()() const { return path; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::text, sqlpp::tag::require_insert>;
    };
    struct Bundle
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "bundle";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T bundle;
            T& operator()() { return bundle; }
            const T& operator()() const { return bundle; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::varchar, sqlpp::tag::require_insert>;
    };
    struct Cluster
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "cluster";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T cluster;
            T& operator()() { return cluster; }
            const T& operator()() const { return cluster; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::varchar, sqlpp::tag::require_insert>;
    };
  } // namespace JobserverFiledownload_

  struct JobserverFiledownload: sqlpp::table_t<JobserverFiledownload,
               JobserverFiledownload_::Id,
               JobserverFiledownload_::User,
               JobserverFiledownload_::Uuid,
               JobserverFiledownload_::Timestamp,
               JobserverFiledownload_::Job,
               JobserverFiledownload_::Path,
               JobserverFiledownload_::Bundle,
               JobserverFiledownload_::Cluster>
  {
    struct _alias_t
    {
      static constexpr const char _literal[] =  "jobserver_filedownload";
      using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
      template<typename T>
      struct _member_t
      {
        T jobserverFiledownload;
        T& operator()() { return jobserverFiledownload; }
        const T& operator()() const { return jobserverFiledownload; }
      };
    };
  };
  namespace JobserverFilelistcache_
  {
    struct Id
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "id";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T id;
            T& operator()() { return id; }
            const T& operator()() const { return id; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::bigint, sqlpp::tag::must_not_insert, sqlpp::tag::must_not_update>;
    };
    struct Timestamp
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "timestamp";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T timestamp;
            T& operator()() { return timestamp; }
            const T& operator()() const { return timestamp; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::time_point, sqlpp::tag::require_insert>;
    };
    struct Path
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "path";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T path;
            T& operator()() { return path; }
            const T& operator()() const { return path; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::varchar, sqlpp::tag::require_insert>;
    };
    struct IsDir
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "is_dir";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T isDir;
            T& operator()() { return isDir; }
            const T& operator()() const { return isDir; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::tinyint, sqlpp::tag::require_insert>;
    };
    struct FileSize
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "file_size";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T fileSize;
            T& operator()() { return fileSize; }
            const T& operator()() const { return fileSize; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::bigint, sqlpp::tag::require_insert>;
    };
    struct Permissions
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "permissions";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T permissions;
            T& operator()() { return permissions; }
            const T& operator()() const { return permissions; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::integer, sqlpp::tag::require_insert>;
    };
    struct JobId
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "job_id";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T jobId;
            T& operator()() { return jobId; }
            const T& operator()() const { return jobId; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::bigint, sqlpp::tag::require_insert>;
    };
  } // namespace JobserverFilelistcache_

  struct JobserverFilelistcache: sqlpp::table_t<JobserverFilelistcache,
               JobserverFilelistcache_::Id,
               JobserverFilelistcache_::Timestamp,
               JobserverFilelistcache_::Path,
               JobserverFilelistcache_::IsDir,
               JobserverFilelistcache_::FileSize,
               JobserverFilelistcache_::Permissions,
               JobserverFilelistcache_::JobId>
  {
    struct _alias_t
    {
      static constexpr const char _literal[] =  "jobserver_filelistcache";
      using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
      template<typename T>
      struct _member_t
      {
        T jobserverFilelistcache;
        T& operator()() { return jobserverFilelistcache; }
        const T& operator()() const { return jobserverFilelistcache; }
      };
    };
  };
  namespace JobserverJob_
  {
    struct Id
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "id";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T id;
            T& operator()() { return id; }
            const T& operator()() const { return id; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::bigint, sqlpp::tag::must_not_insert, sqlpp::tag::must_not_update>;
    };
    struct Parameters
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "parameters";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T parameters;
            T& operator()() { return parameters; }
            const T& operator()() const { return parameters; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::text, sqlpp::tag::require_insert>;
    };
    struct User
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "user";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T user;
            T& operator()() { return user; }
            const T& operator()() const { return user; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::bigint, sqlpp::tag::require_insert>;
    };
    struct Bundle
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "bundle";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T bundle;
            T& operator()() { return bundle; }
            const T& operator()() const { return bundle; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::varchar, sqlpp::tag::require_insert>;
    };
    struct Cluster
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "cluster";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T cluster;
            T& operator()() { return cluster; }
            const T& operator()() const { return cluster; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::varchar, sqlpp::tag::require_insert>;
    };
    struct Application
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "application";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T application;
            T& operator()() { return application; }
            const T& operator()() const { return application; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::varchar, sqlpp::tag::require_insert>;
    };
  } // namespace JobserverJob_

  struct JobserverJob: sqlpp::table_t<JobserverJob,
               JobserverJob_::Id,
               JobserverJob_::Parameters,
               JobserverJob_::User,
               JobserverJob_::Bundle,
               JobserverJob_::Cluster,
               JobserverJob_::Application>
  {
    struct _alias_t
    {
      static constexpr const char _literal[] =  "jobserver_job";
      using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
      template<typename T>
      struct _member_t
      {
        T jobserverJob;
        T& operator()() { return jobserverJob; }
        const T& operator()() const { return jobserverJob; }
      };
    };
  };
  namespace JobserverJobhistory_
  {
    struct Id
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "id";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T id;
            T& operator()() { return id; }
            const T& operator()() const { return id; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::bigint, sqlpp::tag::must_not_insert, sqlpp::tag::must_not_update>;
    };
    struct Timestamp
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "timestamp";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T timestamp;
            T& operator()() { return timestamp; }
            const T& operator()() const { return timestamp; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::time_point, sqlpp::tag::require_insert>;
    };
    struct State
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "state";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T state;
            T& operator()() { return state; }
            const T& operator()() const { return state; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::integer, sqlpp::tag::require_insert>;
    };
    struct Details
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "details";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T details;
            T& operator()() { return details; }
            const T& operator()() const { return details; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::text, sqlpp::tag::require_insert>;
    };
    struct JobId
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "job_id";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T jobId;
            T& operator()() { return jobId; }
            const T& operator()() const { return jobId; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::bigint, sqlpp::tag::require_insert>;
    };
    struct What
    {
      struct _alias_t
      {
        static constexpr const char _literal[] =  "what";
        using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
        template<typename T>
        struct _member_t
          {
            T what;
            T& operator()() { return what; }
            const T& operator()() const { return what; }
          };
      };
      using _traits = sqlpp::make_traits<sqlpp::varchar, sqlpp::tag::require_insert>;
    };
  } // namespace JobserverJobhistory_

  struct JobserverJobhistory: sqlpp::table_t<JobserverJobhistory,
               JobserverJobhistory_::Id,
               JobserverJobhistory_::Timestamp,
               JobserverJobhistory_::State,
               JobserverJobhistory_::Details,
               JobserverJobhistory_::JobId,
               JobserverJobhistory_::What>
  {
    struct _alias_t
    {
      static constexpr const char _literal[] =  "jobserver_jobhistory";
      using _name_t = sqlpp::make_char_sequence<sizeof(_literal), _literal>;
      template<typename T>
      struct _member_t
      {
        T jobserverJobhistory;
        T& operator()() { return jobserverJobhistory; }
        const T& operator()() const { return jobserverJobhistory; }
      };
    };
  };
} // namespace schema
#endif
// NOLINTEND
