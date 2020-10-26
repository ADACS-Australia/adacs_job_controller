-- MariaDB dump 10.17  Distrib 10.4.13-MariaDB, for Linux (x86_64)
--
-- Host: localhost    Database: jobserver
-- ------------------------------------------------------
-- Server version	10.4.13-MariaDB-log

/*!40101 SET @OLD_CHARACTER_SET_CLIENT=@@CHARACTER_SET_CLIENT */;
/*!40101 SET @OLD_CHARACTER_SET_RESULTS=@@CHARACTER_SET_RESULTS */;
/*!40101 SET @OLD_COLLATION_CONNECTION=@@COLLATION_CONNECTION */;
/*!40101 SET NAMES utf8 */;
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
  `app` varchar(255) NOT NULL,
  `name` varchar(255) NOT NULL,
  `applied` datetime(6) NOT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=16 DEFAULT CHARSET=utf8;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `jobserver_clusteruuid`
--

DROP TABLE IF EXISTS `jobserver_clusteruuid`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `jobserver_clusteruuid` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `cluster` varchar(200) NOT NULL,
  `uuid` varchar(36) NOT NULL,
  `timestamp` datetime(6) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `uuid` (`uuid`),
  KEY `jobserver_clusteruuid_timestamp_8f6c293c` (`timestamp`)
) ENGINE=InnoDB AUTO_INCREMENT=320 DEFAULT CHARSET=utf8;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `jobserver_filedownload`
--

DROP TABLE IF EXISTS `jobserver_filedownload`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `jobserver_filedownload` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `user` int(11) NOT NULL,
  `uuid` varchar(36) NOT NULL,
  `timestamp` datetime(6) NOT NULL,
  `job_id` int(11) NOT NULL,
  `path` longtext NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `jobserver_filedownload_uuid_20de5691_uniq` (`uuid`),
  KEY `jobserver_filedownload_job_id_4ff13ecc_fk_jobserver_job_id` (`job_id`),
  KEY `jobserver_filedownload_timestamp_f4d33e0e` (`timestamp`),
  CONSTRAINT `jobserver_filedownload_job_id_4ff13ecc_fk_jobserver_job_id` FOREIGN KEY (`job_id`) REFERENCES `jobserver_job` (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `jobserver_job`
--

DROP TABLE IF EXISTS `jobserver_job`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `jobserver_job` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `parameters` longtext NOT NULL,
  `user` int(11) NOT NULL,
  `bundle` varchar(40) NOT NULL,
  `cluster` varchar(200) NOT NULL,
  `application` varchar(32) NOT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `jobserver_jobhistory`
--

DROP TABLE IF EXISTS `jobserver_jobhistory`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8 */;
CREATE TABLE `jobserver_jobhistory` (
  `id` int(11) NOT NULL AUTO_INCREMENT,
  `timestamp` datetime(6) NOT NULL,
  `state` int(11) NOT NULL,
  `details` longtext NOT NULL,
  `job_id` int(11) NOT NULL,
  `what` varchar(128) NOT NULL,
  PRIMARY KEY (`id`),
  KEY `jobserver_jobhistory_job_id_01bbd7b0_fk_jobserver_job_id` (`job_id`),
  KEY `jobserver_jobhistory_state_e95ac866` (`state`),
  KEY `jobserver_jobhistory_timestamp_d038c893` (`timestamp`),
  KEY `jobserver_jobhistory_what_911845fe` (`what`),
  CONSTRAINT `jobserver_jobhistory_job_id_01bbd7b0_fk_jobserver_job_id` FOREIGN KEY (`job_id`) REFERENCES `jobserver_job` (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8;
/*!40101 SET character_set_client = @saved_cs_client */;
/*!40103 SET TIME_ZONE=@OLD_TIME_ZONE */;

/*!40101 SET SQL_MODE=@OLD_SQL_MODE */;
/*!40014 SET FOREIGN_KEY_CHECKS=@OLD_FOREIGN_KEY_CHECKS */;
/*!40014 SET UNIQUE_CHECKS=@OLD_UNIQUE_CHECKS */;
/*!40101 SET CHARACTER_SET_CLIENT=@OLD_CHARACTER_SET_CLIENT */;
/*!40101 SET CHARACTER_SET_RESULTS=@OLD_CHARACTER_SET_RESULTS */;
/*!40101 SET COLLATION_CONNECTION=@OLD_COLLATION_CONNECTION */;
/*!40111 SET SQL_NOTES=@OLD_SQL_NOTES */;

-- Dump completed on 2020-10-21 14:18:39
