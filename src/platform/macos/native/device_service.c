#include "shim_common.h"

int open_cd_raw_device(const char *bsdName) {
    if (!bsdName || bsdName[0] == '\0') {
        errno = ENODEV;
        return -1;
    }

    char path[96];
    if (strncmp(bsdName, "/dev/rdisk", 10) == 0) {
        snprintf(path, sizeof(path), "%s", bsdName);
    } else if (strncmp(bsdName, "/dev/disk", 9) == 0) {
        snprintf(path, sizeof(path), "/dev/r%s", bsdName + 5);
    } else if (strncmp(bsdName, "rdisk", 5) == 0) {
        snprintf(path, sizeof(path), "/dev/%s", bsdName);
    } else {
        snprintf(path, sizeof(path), "/dev/r%s", bsdName);
    }

    // important not to open with a write flag so it does not need exclusivity
    return open(path, O_RDONLY | O_NONBLOCK);
}
