#include "shim_common.h"

static uint8_t task_status_to_scsi_status(SCSITaskStatus status) {
    switch (status) {
        case kSCSITaskStatus_GOOD: return 0x00;
        case kSCSITaskStatus_CHECK_CONDITION: return 0x02;
        case kSCSITaskStatus_BUSY: return 0x08;
        case kSCSITaskStatus_RESERVATION_CONFLICT: return 0x18;
        case kSCSITaskStatus_TASK_SET_FULL: return 0x28;
        case kSCSITaskStatus_ACA_ACTIVE: return 0x30;
        default: return 0xFF;
    }
}

static void fill_scsi_error(CdScsiError *outErr, kern_return_t ex, SCSITaskStatus status, SCSI_Sense_Data *sense) {
    if (!outErr) return;

    outErr->has_scsi_error = 1;
    outErr->exec_error = (uint32_t)ex;
    outErr->task_status = (uint32_t)status;
    outErr->scsi_status = task_status_to_scsi_status(status);

    const uint8_t *sense_bytes = (const uint8_t *)sense;
    bool has_sense = false;
    for (size_t i = 0; i < sizeof(SCSI_Sense_Data); i++) {
        if (sense_bytes[i] != 0) {
            has_sense = true;
            break;
        }
    }

    outErr->has_sense = has_sense ? 1 : 0;
    if (has_sense && sizeof(SCSI_Sense_Data) >= 14) {
        outErr->sense_key = sense_bytes[2] & 0x0F;
        outErr->asc = sense_bytes[12];
        outErr->ascq = sense_bytes[13];
    }
}

bool read_cd_audio(uint32_t lba, uint32_t sectors, uint8_t **outBuf, uint32_t *outLen, CdScsiError *outErr) {
    *outBuf = NULL;
    *outLen = 0;
    if (outErr) {
        memset(outErr, 0, sizeof(CdScsiError));
    }

    SCSITaskDeviceInterface **dev = globalDev;
    if (!dev) {
        fprintf(stderr, "[READ] Device session is not open\n");
        goto fail;
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

    const uint32_t MAX_SECTORS_PER_CMD = 27;

    uint32_t remaining = sectors;
    uint32_t curLBA = lba;
    uint32_t written = 0;

    while (remaining > 0) {
        uint32_t xfer = (remaining > MAX_SECTORS_PER_CMD) ? MAX_SECTORS_PER_CMD : remaining;
        uint32_t bytes = xfer * SECTOR_SZ;

        // READ CD (0xBE) 12-byte CDB for CD-DA.
        uint8_t cdb[12] = {0};
        cdb[0] = 0xBE;
        cdb[1] = 0x00;
        cdb[2] = (uint8_t)((curLBA >> 24) & 0xFF);
        cdb[3] = (uint8_t)((curLBA >> 16) & 0xFF);
        cdb[4] = (uint8_t)((curLBA >> 8) & 0xFF);
        cdb[5] = (uint8_t)((curLBA >> 0) & 0xFF);
        cdb[6] = (uint8_t)((xfer >> 16) & 0xFF);
        cdb[7] = (uint8_t)((xfer >> 8) & 0xFF);
        cdb[8] = (uint8_t)((xfer >> 0) & 0xFF);
        cdb[9] = 0x10; // USER DATA only (2352 bytes/sector)
        cdb[10] = 0x00;
        cdb[11] = 0x00;

        SCSITaskInterface **task = (*dev)->CreateSCSITask(dev);
        if (!task) {
            fprintf(stderr, "[READ] CreateSCSITask failed\n");
            free(dst);
            goto fail;
        }

        IOVirtualRange vr = {0};
        vr.address = (IOVirtualAddress)(dst + written);
        vr.length = bytes;

        if ((*task)->SetCommandDescriptorBlock(task, cdb, sizeof(cdb)) != kIOReturnSuccess) {
            fprintf(stderr, "[READ] SetCommandDescriptorBlock failed\n");
            (*task)->Release(task);
            free(dst);
            goto fail;
        }

        // dir=2 means from device in SCSITaskLib.
        if ((*task)->SetScatterGatherEntries(task, &vr, 1, bytes, 2) != kIOReturnSuccess) {
            fprintf(stderr, "[READ] SetScatterGatherEntries failed\n");
            (*task)->Release(task);
            free(dst);
            goto fail;
        }

        SCSI_Sense_Data sense = {0};
        SCSITaskStatus status = kSCSITaskStatus_No_Status;
        kern_return_t ex = (*task)->ExecuteTaskSync(task, &sense, &status, NULL);
        (*task)->Release(task);

        if (ex != kIOReturnSuccess || status != kSCSITaskStatus_GOOD) {
            fill_scsi_error(outErr, ex, status, &sense);
            fprintf(stderr, "[READ] ExecuteTaskSync failed (ex=0x%x, status=%u)\n", ex, status);
            free(dst);
            goto fail;
        }

        written += bytes;
        curLBA += xfer;
        remaining -= xfer;
    }

    *outBuf = dst;
    *outLen = written;

    return true;

fail:
    return false;
}
