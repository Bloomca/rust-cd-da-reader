#include "shim_common.h"

bool cd_read_track_information(int fd, uint8_t trackNumber,
                               uint8_t **outBuf, uint32_t *outLen,
                               CdScsiError *outErr) {
    if (!outBuf || !outLen) {
        return false;
    }

    *outBuf = NULL;
    *outLen = 0;
    if (outErr) {
        memset(outErr, 0, sizeof(CdScsiError));
    }

    const uint16_t rawAlloc = sizeof(CDTrackInfo);
    uint8_t *raw = calloc(1, rawAlloc);
    if (!raw) {
        fprintf(stderr, "[TRACK INFO] oom\n");
        return false;
    }

    dk_cd_read_track_info_t request = {0};
    request.address = trackNumber;
    request.addressType = kCDTrackInfoAddressTypeTrackNumber;
    request.bufferLength = rawAlloc;
    request.buffer = raw;

    int ret = ioctl(fd, DKIOCCDREADTRACKINFO, &request);
    if (ret < 0) {
        fprintf(stderr, "[TRACK INFO] DKIOCCDREADTRACKINFO failed (errno=%d)\n", errno);
        free(raw);
        return false;
    }

    if (request.bufferLength < 32) {
        fprintf(stderr, "[TRACK INFO] response is too short\n");
        free(raw);
        return false;
    }

    uint16_t dataLength = OSSwapBigToHostInt16(((CDTrackInfo *)raw)->dataLength);
    uint32_t responseLength = (uint32_t)dataLength + sizeof(uint16_t);
    if (responseLength < 32 || responseLength > request.bufferLength) {
        fprintf(stderr, "[TRACK INFO] response length is invalid\n");
        free(raw);
        return false;
    }

    uint8_t *result = malloc(responseLength);
    if (!result) {
        fprintf(stderr, "[TRACK INFO] oom\n");
        free(raw);
        return false;
    }

    memcpy(result, raw, responseLength);
    free(raw);

    *outBuf = result;
    *outLen = responseLength;
    return true;
}
