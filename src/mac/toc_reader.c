#include "shim_common.h"

static Boolean read_toc(uint8_t **outBuf, uint32_t *outLen, CdScsiError *outErr) {
    *outBuf = NULL;
    *outLen = 0;
    if (outErr) {
        memset(outErr, 0, sizeof(CdScsiError));
    }

    int fd = open_cd_raw_device();
    if (fd < 0) {
        fprintf(stderr, "[TOC] open raw CD device failed (errno=%d)\n", errno);
        goto fail;
    }

    const uint16_t rawAlloc = 4096;
    uint8_t *raw = malloc(rawAlloc);
    if (!raw) {
        close(fd);
        fprintf(stderr, "[TOC] oom\n");
        goto fail;
    }

    dk_cd_read_toc_t request = {0};
    request.format = kCDTOCFormatTOC;
    request.formatAsTime = 1;
    request.address.session = 0;
    request.bufferLength = rawAlloc;
    request.buffer = raw;

    int ret = ioctl(fd, DKIOCCDREADTOC, &request);
    close(fd);

    if (ret < 0) {
        fprintf(stderr, "[TOC] DKIOCCDREADTOC failed (errno=%d)\n", errno);
        free(raw);
        goto fail;
    }

    if (request.bufferLength < sizeof(CDTOC)) {
        fprintf(stderr, "[TOC] returned TOC is too short\n");
        free(raw);
        goto fail;
    }

    CDTOC *toc = (CDTOC *)raw;
    uint16_t tocLength = OSSwapBigToHostInt16(toc->length);
    size_t tocSize = (size_t)tocLength + sizeof(toc->length);
    if (tocSize > request.bufferLength) {
        fprintf(stderr, "[TOC] returned TOC length is invalid\n");
        free(raw);
        goto fail;
    }

    CDTOCDescriptor *tracks[100] = {0};
    CDTOCDescriptor *leadout = NULL;
    uint8_t firstTrack = 0;
    uint8_t lastTrack = 0;
    uint32_t trackCount = 0;

    UInt32 descriptorCount = CDTOCGetDescriptorCount(toc);
    for (UInt32 i = 0; i < descriptorCount; i++) {
        CDTOCDescriptor *desc = &toc->descriptors[i];
        if (desc->adr != 1) {
            continue;
        }

        if (desc->point >= 1 && desc->point <= 99) {
            if (!tracks[desc->point]) {
                trackCount++;
            }
            tracks[desc->point] = desc;
            if (firstTrack == 0 || desc->point < firstTrack) {
                firstTrack = desc->point;
            }
            if (desc->point > lastTrack) {
                lastTrack = desc->point;
            }
        } else if (desc->point == 0xA2) {
            leadout = desc;
        }
    }

    if (trackCount == 0 || !leadout) {
        fprintf(stderr, "[TOC] did not find track descriptors and leadout\n");
        free(raw);
        goto fail;
    }

    uint32_t outputDescriptors = trackCount + 1;
    uint16_t outputTocLength = (uint16_t)(2 + outputDescriptors * 8);
    uint32_t outputLen = (uint32_t)outputTocLength + sizeof(uint16_t);
    uint8_t *buf = malloc(outputLen);
    if (!buf) {
        free(raw);
        fprintf(stderr, "[TOC] oom\n");
        goto fail;
    }
    memset(buf, 0, outputLen);

    buf[0] = (uint8_t)((outputTocLength >> 8) & 0xFF);
    buf[1] = (uint8_t)(outputTocLength & 0xFF);
    buf[2] = firstTrack;
    buf[3] = lastTrack;

    uint32_t offset = 4;
    for (uint8_t track = firstTrack; track <= lastTrack; track++) {
        CDTOCDescriptor *desc = tracks[track];
        if (!desc) {
            continue;
        }

        uint32_t lba = CDConvertMSFToLBA(desc->p);
        buf[offset + 1] = (uint8_t)((desc->adr << 4) | desc->control);
        buf[offset + 2] = track;
        buf[offset + 4] = (uint8_t)((lba >> 24) & 0xFF);
        buf[offset + 5] = (uint8_t)((lba >> 16) & 0xFF);
        buf[offset + 6] = (uint8_t)((lba >> 8) & 0xFF);
        buf[offset + 7] = (uint8_t)(lba & 0xFF);
        offset += 8;
    }

    uint32_t leadoutLba = CDConvertMSFToLBA(leadout->p);
    buf[offset + 1] = (uint8_t)((leadout->adr << 4) | leadout->control);
    buf[offset + 2] = 0xAA;
    buf[offset + 4] = (uint8_t)((leadoutLba >> 24) & 0xFF);
    buf[offset + 5] = (uint8_t)((leadoutLba >> 16) & 0xFF);
    buf[offset + 6] = (uint8_t)((leadoutLba >> 8) & 0xFF);
    buf[offset + 7] = (uint8_t)(leadoutLba & 0xFF);

    free(raw);

    *outBuf = buf;
    *outLen = outputLen;
    return true;

fail:
    return false;
}

bool cd_read_toc(uint8_t **outBuf, uint32_t *outLen, CdScsiError *outErr) {
    return read_toc(outBuf, outLen, outErr);
}

void cd_free(void *p) {
    if (p) free(p);
}
