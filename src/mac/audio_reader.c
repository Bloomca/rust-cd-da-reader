#include "shim_common.h"

bool read_cd_audio(uint32_t lba, uint32_t sectors, uint8_t **outBuf, uint32_t *outLen, CdScsiError *outErr) {
    *outBuf = NULL;
    *outLen = 0;
    if (outErr) {
        memset(outErr, 0, sizeof(CdScsiError));
    }

    const uint32_t SECTOR_SZ = 2352;
    if (sectors == 0) {
        fprintf(stderr, "[READ] sectors == 0\n");
        goto fail;
    }

    uint64_t totalBytes64 = (uint64_t)SECTOR_SZ * (uint64_t)sectors;
    if (totalBytes64 > UINT32_MAX) {
        fprintf(stderr, "[READ] requested size too large\n");
        goto fail;
    }
    uint32_t totalBytes = (uint32_t)totalBytes64;

    uint8_t *dst = (uint8_t *)malloc(totalBytes);
    if (!dst) {
        fprintf(stderr, "[READ] oom\n");
        goto fail;
    }

    int fd = open_cd_raw_device();
    if (fd < 0) {
        fprintf(stderr, "[READ] open raw CD device failed (errno=%d)\n", errno);
        free(dst);
        goto fail;
    }

    dk_cd_read_t read = {0};
    read.offset = (uint64_t)lba * (uint64_t)SECTOR_SZ;
    read.sectorArea = kCDSectorAreaUser;
    read.sectorType = kCDSectorTypeCDDA;
    read.bufferLength = totalBytes;
    read.buffer = dst;

    int ret = ioctl(fd, DKIOCCDREAD, &read);
    close(fd);

    if (ret < 0) {
        fprintf(stderr, "[READ] DKIOCCDREAD failed (errno=%d)\n", errno);
        free(dst);
        goto fail;
    }

    if (read.bufferLength != totalBytes) {
        fprintf(stderr, "[READ] short read: requested=%u actual=%u\n", totalBytes, read.bufferLength);
        free(dst);
        goto fail;
    }

    *outBuf = dst;
    *outLen = totalBytes;

    return true;

fail:
    return false;
}
