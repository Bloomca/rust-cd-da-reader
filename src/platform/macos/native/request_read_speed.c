#include "shim_common.h"
#include <IOKit/storage/IOCDMediaBSDClient.h>

Boolean request_cd_read_speed(int fd, uint16_t target_speed_kbs) {
    int ret = ioctl(fd, DKIOCCDSETSPEED, target_speed_kbs);
    if (ret < 0) {
        fprintf(stderr, "[SPEED] DKIOCCDSETSPEED failed (errno=%d\n)", errno);
        return false;
    }

    return true;
}
