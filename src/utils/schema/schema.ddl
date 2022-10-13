-- MariaDB dump 10.19  Distrib 10.9.3-MariaDB, for Linux (x86_64)
--
-- Host: localhost    Database: jobserver
-- ------------------------------------------------------
-- Server version	10.9.3-MariaDB

/*!40101 SET @OLD_CHARACTER_SET_CLIENT=@@CHARACTER_SET_CLIENT */;
/*!40101 SET @OLD_CHARACTER_SET_RESULTS=@@CHARACTER_SET_RESULTS */;
/*!40101 SET @OLD_COLLATION_CONNECTION=@@COLLATION_CONNECTION */;
/*!40101 SET NAMES utf8mb4 */;
/*!40103 SET @OLD_TIME_ZONE=@@TIME_ZONE */;
/*!40103 SET TIME_ZONE='+00:00' */;
/*!40014 SET @OLD_UNIQUE_CHECKS=@@UNIQUE_CHECKS, UNIQUE_CHECKS=0 */;
/*!40014 SET @OLD_FOREIGN_KEY_CHECKS=@@FOREIGN_KEY_CHECKS, FOREIGN_KEY_CHECKS=0 */;
/*!40101 SET @OLD_SQL_MODE=@@SQL_MODE, SQL_MODE='NO_AUTO_VALUE_ON_ZERO' */;
/*!40111 SET @OLD_SQL_NOTES=@@SQL_NOTES, SQL_NOTES=0 */;

--
-- Table structure for table `django_migrations`
--

DROP TABLE IF EXISTS `django_migrations`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `django_migrations` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `app` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `name` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `applied` datetime(6) NOT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=27 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `jobclient_job`
--

DROP TABLE IF EXISTS `jobclient_job`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `jobclient_job` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT,
  `job_id` bigint(20) DEFAULT NULL,
  `scheduler_id` bigint(20) DEFAULT NULL,
  `submitting` tinyint(1) NOT NULL,
  `submitting_count` int(11) NOT NULL,
  `bundle_hash` varchar(40) COLLATE utf8mb4_unicode_ci NOT NULL,
  `working_directory` varchar(512) COLLATE utf8mb4_unicode_ci NOT NULL,
  `queued` tinyint(1) NOT NULL,
  `params` longtext COLLATE utf8mb4_unicode_ci NOT NULL,
  `running` tinyint(1) NOT NULL,
  PRIMARY KEY (`id`),
  KEY `jobclient_job_job_id_b6cab13c` (`job_id`),
  KEY `jobclient_job_scheduler_id_46e6b6a1` (`scheduler_id`),
  KEY `jobclient_job_queued_f6d27061` (`queued`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `jobclient_jobstatusmodel`
--

DROP TABLE IF EXISTS `jobclient_jobstatusmodel`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `jobclient_jobstatusmodel` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT,
  `what` varchar(128) COLLATE utf8mb4_unicode_ci NOT NULL,
  `state` int(11) NOT NULL,
  `job_id` bigint(20) NOT NULL,
  PRIMARY KEY (`id`),
  KEY `jobclient_jobstatusmodel_job_id_743286d0_fk_jobclient_job_id` (`job_id`),
  KEY `jobclient_jobstatusmodel_state_1d61edb3` (`state`),
  CONSTRAINT `jobclient_jobstatusmodel_job_id_743286d0_fk_jobclient_job_id` FOREIGN KEY (`job_id`) REFERENCES `jobclient_job` (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `jobserver_bundlejob`
--

DROP TABLE IF EXISTS `jobserver_bundlejob`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `jobserver_bundlejob` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT,
  `cluster` varchar(200) COLLATE utf8mb4_unicode_ci NOT NULL,
  `bundle_hash` varchar(40) COLLATE utf8mb4_unicode_ci NOT NULL,
  `content` longtext COLLATE utf8mb4_unicode_ci NOT NULL,
  PRIMARY KEY (`id`),
  KEY `jobserver_bundlejob_cluster_5281feda` (`cluster`),
  KEY `jobserver_bundlejob_bundle_hash_e0a2273a` (`bundle_hash`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `jobserver_clusterjob`
--

DROP TABLE IF EXISTS `jobserver_clusterjob`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `jobserver_clusterjob` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT,
  `cluster` varchar(200) COLLATE utf8mb4_unicode_ci NOT NULL,
  `job_id` int(11) DEFAULT NULL,
  `scheduler_id` int(11) DEFAULT NULL,
  `submitting` tinyint(1) NOT NULL,
  `submitting_count` int(11) NOT NULL,
  `bundle_hash` varchar(40) COLLATE utf8mb4_unicode_ci NOT NULL,
  `working_directory` varchar(512) COLLATE utf8mb4_unicode_ci NOT NULL,
  `running` tinyint(1) NOT NULL,
  PRIMARY KEY (`id`),
  KEY `jobserver_clusterjob_cluster_58bbe756` (`cluster`),
  KEY `jobserver_clusterjob_job_id_1288b3ad` (`job_id`),
  KEY `jobserver_clusterjob_scheduler_id_d9aeaa21` (`scheduler_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `jobserver_clusterjobstatus`
--

DROP TABLE IF EXISTS `jobserver_clusterjobstatus`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `jobserver_clusterjobstatus` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT,
  `what` varchar(128) COLLATE utf8mb4_unicode_ci NOT NULL,
  `state` int(11) NOT NULL,
  `job_id` bigint(20) NOT NULL,
  PRIMARY KEY (`id`),
  KEY `jobserver_clusterjob_job_id_017cf8a8_fk_jobserver` (`job_id`),
  KEY `jobserver_clusterjobstatus_state_99744516` (`state`),
  CONSTRAINT `jobserver_clusterjob_job_id_017cf8a8_fk_jobserver` FOREIGN KEY (`job_id`) REFERENCES `jobserver_clusterjob` (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `jobserver_clusteruuid`
--

DROP TABLE IF EXISTS `jobserver_clusteruuid`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `jobserver_clusteruuid` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT,
  `cluster` varchar(200) COLLATE utf8mb4_unicode_ci NOT NULL,
  `uuid` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `timestamp` datetime(6) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `uuid` (`uuid`),
  KEY `jobserver_clusteruuid_timestamp_8f6c293c` (`timestamp`)
) ENGINE=InnoDB AUTO_INCREMENT=13929 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `jobserver_filedownload`
--

DROP TABLE IF EXISTS `jobserver_filedownload`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `jobserver_filedownload` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT,
  `user` bigint(20) NOT NULL,
  `uuid` varchar(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `timestamp` datetime(6) NOT NULL,
  `job` bigint(20) NOT NULL,
  `path` longtext COLLATE utf8mb4_unicode_ci NOT NULL,
  `bundle` varchar(40) COLLATE utf8mb4_unicode_ci NOT NULL,
  `cluster` varchar(200) COLLATE utf8mb4_unicode_ci NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `jobserver_filedownload_uuid_20de5691_uniq` (`uuid`),
  KEY `jobserver_filedownload_timestamp_f4d33e0e` (`timestamp`)
) ENGINE=InnoDB AUTO_INCREMENT=1914 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `jobserver_filelistcache`
--

DROP TABLE IF EXISTS `jobserver_filelistcache`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `jobserver_filelistcache` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT,
  `timestamp` datetime(6) NOT NULL,
  `path` varchar(765) COLLATE utf8mb4_unicode_ci NOT NULL,
  `is_dir` tinyint(1) NOT NULL,
  `file_size` bigint(20) NOT NULL,
  `permissions` int(11) NOT NULL,
  `job_id` bigint(20) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `jobserver_filelistcache_job_id_path_0ffc12e3_uniq` (`job_id`,`path`),
  KEY `jobserver_filelistcache_timestamp_c08291f7` (`timestamp`),
  KEY `jobserver_filelistcache_path_92e327d3` (`path`),
  CONSTRAINT `jobserver_filelistcache_job_id_04ab005b_fk` FOREIGN KEY (`job_id`) REFERENCES `jobserver_job` (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=2800 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `jobserver_job`
--

DROP TABLE IF EXISTS `jobserver_job`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `jobserver_job` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT,
  `parameters` longtext COLLATE utf8mb4_unicode_ci NOT NULL,
  `user` bigint(20) NOT NULL,
  `bundle` varchar(40) COLLATE utf8mb4_unicode_ci NOT NULL,
  `cluster` varchar(200) COLLATE utf8mb4_unicode_ci NOT NULL,
  `application` varchar(32) COLLATE utf8mb4_unicode_ci NOT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=11261 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `jobserver_jobhistory`
--

DROP TABLE IF EXISTS `jobserver_jobhistory`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `jobserver_jobhistory` (
  `id` bigint(20) NOT NULL AUTO_INCREMENT,
  `timestamp` datetime(6) NOT NULL,
  `state` int(11) NOT NULL,
  `details` longtext COLLATE utf8mb4_unicode_ci NOT NULL,
  `job_id` bigint(20) NOT NULL,
  `what` varchar(128) COLLATE utf8mb4_unicode_ci NOT NULL,
  PRIMARY KEY (`id`),
  KEY `jobserver_jobhistory_state_e95ac866` (`state`),
  KEY `jobserver_jobhistory_timestamp_d038c893` (`timestamp`),
  KEY `jobserver_jobhistory_what_911845fe` (`what`),
  KEY `jobserver_jobhistory_job_id_01bbd7b0_fk` (`job_id`),
  CONSTRAINT `jobserver_jobhistory_job_id_01bbd7b0_fk` FOREIGN KEY (`job_id`) REFERENCES `jobserver_job` (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=26305 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;
/*!40103 SET TIME_ZONE=@OLD_TIME_ZONE */;

/*!40101 SET SQL_MODE=@OLD_SQL_MODE */;
/*!40014 SET FOREIGN_KEY_CHECKS=@OLD_FOREIGN_KEY_CHECKS */;
/*!40014 SET UNIQUE_CHECKS=@OLD_UNIQUE_CHECKS */;
/*!40101 SET CHARACTER_SET_CLIENT=@OLD_CHARACTER_SET_CLIENT */;
/*!40101 SET CHARACTER_SET_RESULTS=@OLD_CHARACTER_SET_RESULTS */;
/*!40101 SET COLLATION_CONNECTION=@OLD_COLLATION_CONNECTION */;
/*!40111 SET SQL_NOTES=@OLD_SQL_NOTES */;

-- Dump completed on 2022-10-13 12:39:10
