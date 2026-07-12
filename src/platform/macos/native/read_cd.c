#include "shim_common.h"

// Map our SectorReadFormat discriminant to the macOS CD sector area/type and
// the resulting bytes-per-sector. Keeping the mapping here (rather than in
// Rust) means the IOKit constants only ever appear where their header is
// imported.
//
//   0 = Audio                 -> user, CDDA, 2352 B/sector
//   1 = Mode1Cooked           -> user, Mode 1, 2048 B/sector
//   2 = Mode1Raw              -> full Mode 1, 2352 B/sector
//   3 = Mode2FormlessCooked   -> user, Mode 2, 2336 B/sector
//   4 = Mode2FormlessRaw      -> full Mode 2, 2352 B/sector
//   5 = Mode2Form1Cooked      -> user, Mode 2 Form 1, 2048 B/sector
//   6 = Mode2Form1Raw         -> full Mode 2 Form 1, 2352 B/sector
//   7 = Mode2Form2Cooked      -> user, Mode 2 Form 2, 2328 B/sector
//   8 = Mode2Form2Raw         -> full Mode 2 Form 2, 2352 B/sector
//   9 = AnyRaw                -> full sector, unknown type, 2352 B/sector
static bool sector_layout_for_format(uint32_t format_id,
                                     CDSectorArea *outArea,
                                     CDSectorType *outType,
                                     uint32_t *outSectorSize) {
    switch (format_id) {
        case 0:
            *outArea = kCDSectorAreaUser;
            *outType = kCDSectorTypeCDDA;
            *outSectorSize = 2352;
            return true;
        case 1:
            *outArea = kCDSectorAreaUser;
            *outType = kCDSectorTypeMode1;
            *outSectorSize = 2048;
            return true;
        case 2:
            *outArea = (CDSectorArea)(kCDSectorAreaSync | kCDSectorAreaHeader |
                                      kCDSectorAreaUser | kCDSectorAreaAuxiliary);
            *outType = kCDSectorTypeMode1;
            *outSectorSize = 2352;
            return true;
        case 3:
            *outArea = kCDSectorAreaUser;
            *outType = kCDSectorTypeMode2;
            *outSectorSize = 2336;
            return true;
        case 4:
            *outArea = (CDSectorArea)(kCDSectorAreaSync | kCDSectorAreaHeader |
                                      kCDSectorAreaUser);
            *outType = kCDSectorTypeMode2;
            *outSectorSize = 2352;
            return true;
        case 5:
            *outArea = kCDSectorAreaUser;
            *outType = kCDSectorTypeMode2Form1;
            *outSectorSize = 2048;
            return true;
        case 6:
            *outArea = (CDSectorArea)(kCDSectorAreaSync | kCDSectorAreaHeader |
                                      kCDSectorAreaSubHeader | kCDSectorAreaUser |
                                      kCDSectorAreaAuxiliary);
            *outType = kCDSectorTypeMode2Form1;
            *outSectorSize = 2352;
            return true;
        case 7:
            *outArea = kCDSectorAreaUser;
            *outType = kCDSectorTypeMode2Form2;
            *outSectorSize = 2328;
            return true;
        case 8:
            *outArea = (CDSectorArea)(kCDSectorAreaSync | kCDSectorAreaHeader |
                                      kCDSectorAreaSubHeader | kCDSectorAreaUser);
            *outType = kCDSectorTypeMode2Form2;
            *outSectorSize = 2352;
            return true;
        case 9:
            *outArea = (CDSectorArea)(kCDSectorAreaSync | kCDSectorAreaHeader |
                                      kCDSectorAreaSubHeader | kCDSectorAreaUser |
                                      kCDSectorAreaAuxiliary);
            *outType = kCDSectorTypeUnknown;
            *outSectorSize = 2352;
            return true;
        default:
            return false;
    }
}

bool read_cd_sectors(int fd, uint32_t lba, uint32_t sectors, uint32_t format_id,
                     uint8_t **outBuf, uint32_t *outLen, CdScsiError *outErr) {
    *outBuf = NULL;
    *outLen = 0;
    if (outErr) {
        memset(outErr, 0, sizeof(CdScsiError));
    }

    CDSectorArea sectorArea;
    CDSectorType sectorType;
    uint32_t sectorSize;
    if (!sector_layout_for_format(format_id, &sectorArea, &sectorType, &sectorSize)) {
        fprintf(stderr, "[READ] unknown sector format %u\n", format_id);
        goto fail;
    }

    if (sectors == 0) {
        fprintf(stderr, "[READ] sectors == 0\n");
        goto fail;
    }

    uint64_t totalBytes64 = (uint64_t)sectorSize * (uint64_t)sectors;
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

    dk_cd_read_t read = {0};
    read.offset = (uint64_t)lba * (uint64_t)sectorSize;
    read.sectorArea = sectorArea;
    read.sectorType = sectorType;
    read.bufferLength = totalBytes;
    read.buffer = dst;

    int ret = ioctl(fd, DKIOCCDREAD, &read);

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
