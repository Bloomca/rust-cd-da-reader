#include "shim_common.h"

static uint8_t task_status_to_scsi_status(SCSITaskStatus status) {
    switch (status) {
        case kSCSITaskStatus_GOOD: return 0x00;
        case kSCSITaskStatus_CHECK_CONDITION: return 0x02;
        case kSCSITaskStatus_BUSY: return 0x08;
        case kSCSITaskStatus_RESERVATION_CONFLICT: return 0x18;
        case kSCSITaskStatus_TASK_SET_FULL: return 0x28;
        case kSCSITaskStatus_ACA_ACTIVE: return 0x30;
        case kSCSITaskStatus_TASK_ABORTED: return 0x40;
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

    SInt32 score = 0;
    IOCFPlugInInterface **plugin = NULL;
    MMCDeviceInterface **mmc = NULL;
    SCSITaskDeviceInterface **dev = NULL;

    io_service_t devSvc = globalDevSvc;
    if (!devSvc && g_guard.bsdName) {
        (void)get_dev_svc(g_guard.bsdName);
        devSvc = globalDevSvc;
    }
    if (!devSvc) {
        fprintf(stderr, "[READ] Could not find mmc device for bsd\n");
        goto fail;
    }

    kern_return_t kret = kIOReturnError;
    for (int attempt = 0; attempt < 2; attempt++) {
        score = 0;
        plugin = NULL;
        kret = IOCreatePlugInInterfaceForService(
            devSvc,
            kIOMMCDeviceUserClientTypeID,
            kIOCFPlugInInterfaceID,
            &plugin,
            &score
        );
        if (kret == kIOReturnSuccess && plugin != NULL) break;

        fprintf(stderr, "[READ] IOCreatePlugInInterfaceForService failed: 0x%x\n", kret);
        if (attempt == 0 && g_guard.bsdName) {
            reset_dev_scv();
            (void)get_dev_svc(g_guard.bsdName);
            devSvc = globalDevSvc;
            if (!devSvc) break;
            continue;
        }
    }
    if (kret != kIOReturnSuccess || plugin == NULL) {
        goto fail;
    }

    HRESULT hr = (*plugin)->QueryInterface(
        plugin,
        CFUUIDGetUUIDBytes(kIOMMCDeviceInterfaceID),
        (LPVOID)&mmc
    );
    if (hr != S_OK || !mmc) {
        fprintf(stderr, "[READ] QueryInterface(kIOMMCDeviceInterfaceID) failed (hr=0x%lx)\n", (long)hr);
        goto fail;
    }

    dev = (*mmc)->GetSCSITaskDeviceInterface(mmc);
    if (!dev) {
        fprintf(stderr, "[READ] GetSCSITaskDeviceInterface failed\n");
        goto fail;
    }

    // As with TOC: unmount externally, then take exclusive access here.
    kret = (*dev)->ObtainExclusiveAccess(dev);
    if (kret != kIOReturnSuccess) {
        if (kret == kIOReturnBusy) {
            fprintf(stderr, "[READ] Busy on obtaining exclusive access\n");
        } else {
            fprintf(stderr, "[READ] ObtainExclusiveAccess error: 0x%x\n", kret);
        }
        goto fail;
    }

    const uint32_t SECTOR_SZ = 2352;
    if (sectors == 0) {
        fprintf(stderr, "[READ] sectors == 0\n");
        goto fail_excl;
    }

    uint64_t totalBytes64 = (uint64_t)SECTOR_SZ * (uint64_t)sectors;
    if (totalBytes64 > UINT32_MAX) {
        fprintf(stderr, "[READ] requested size too large\n");
        goto fail_excl;
    }
    uint32_t totalBytes = (uint32_t)totalBytes64;

    uint8_t *dst = (uint8_t *)malloc(totalBytes);
    if (!dst) {
        fprintf(stderr, "[READ] oom\n");
        goto fail_excl;
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
            goto fail_excl;
        }

        IOVirtualRange vr = {0};
        vr.address = (IOVirtualAddress)(dst + written);
        vr.length = bytes;

        if ((*task)->SetCommandDescriptorBlock(task, cdb, sizeof(cdb)) != kIOReturnSuccess) {
            fprintf(stderr, "[READ] SetCommandDescriptorBlock failed\n");
            (*task)->Release(task);
            free(dst);
            goto fail_excl;
        }

        // dir=2 means from device in SCSITaskLib.
        if ((*task)->SetScatterGatherEntries(task, &vr, 1, bytes, 2) != kIOReturnSuccess) {
            fprintf(stderr, "[READ] SetScatterGatherEntries failed\n");
            (*task)->Release(task);
            free(dst);
            goto fail_excl;
        }

        SCSI_Sense_Data sense = {0};
        SCSITaskStatus status = kSCSITaskStatus_No_Status;
        kern_return_t ex = (*task)->ExecuteTaskSync(task, &sense, &status, NULL);
        (*task)->Release(task);

        if (ex != kIOReturnSuccess || status != kSCSITaskStatus_GOOD) {
            fill_scsi_error(outErr, ex, status, &sense);
            fprintf(stderr, "[READ] ExecuteTaskSync failed (ex=0x%x, status=%u)\n", ex, status);
            free(dst);
            goto fail_excl;
        }

        written += bytes;
        curLBA += xfer;
        remaining -= xfer;
    }

    *outBuf = dst;
    *outLen = written;

    (*dev)->ReleaseExclusiveAccess(dev);
    (*mmc)->Release(mmc);
    IODestroyPlugInInterface(plugin);
    return true;

fail_excl:
    if (dev) (*dev)->ReleaseExclusiveAccess(dev);
fail:
    if (mmc) (*mmc)->Release(mmc);
    if (plugin) IODestroyPlugInInterface(plugin);
    return false;
}
