#include "shim_common.h"

char globalBsdName[64] = {0};

int open_cd_raw_device(void) {
    if (globalBsdName[0] == '\0') {
        errno = ENODEV;
        return -1;
    }

    char path[96];
    if (strncmp(globalBsdName, "/dev/rdisk", 10) == 0) {
        snprintf(path, sizeof(path), "%s", globalBsdName);
    } else if (strncmp(globalBsdName, "/dev/disk", 9) == 0) {
        snprintf(path, sizeof(path), "/dev/r%s", globalBsdName + 5);
    } else if (strncmp(globalBsdName, "rdisk", 5) == 0) {
        snprintf(path, sizeof(path), "/dev/%s", globalBsdName);
    } else {
        snprintf(path, sizeof(path), "/dev/r%s", globalBsdName);
    }

    return open(path, O_RDONLY | O_NONBLOCK);
}

Boolean open_dev_session(const char *bsdName) {
    if (!bsdName || bsdName[0] == '\0') {
        return false;
    }

    if (globalBsdName[0] != '\0') {
        return strcmp(globalBsdName, bsdName) == 0;
    }

    snprintf(globalBsdName, sizeof(globalBsdName), "%s", bsdName);
    return true;
}

void close_dev_session(void) {
    globalBsdName[0] = '\0';
}
