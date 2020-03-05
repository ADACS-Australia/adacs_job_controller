//
// Created by lewis on 3/6/20.
//

#ifndef GWCLOUD_JOB_SERVER_SETTINGS_H
#define GWCLOUD_JOB_SERVER_SETTINGS_H

#define DATABASE_USER               (std::getenv("DATABASE_USER") || "jobserver")
#define DATABASE_PASSWORD           (std::getenv("DATABASE_PASSWORD") || "jobserver")
#define DATABASE_SCHEMA             (std::getenv("DATABASE_SCHEMA") || "jobserver")
#define DATABASE_HOST               (std::getenv("DATABASE_HOST") || "localhost")
#define DATABASE_PORT               (std::getenv("DATABASE_PORT") || "localhost")

#endif //GWCLOUD_JOB_SERVER_SETTINGS_H
