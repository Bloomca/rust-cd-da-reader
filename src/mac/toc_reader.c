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

static Boolean read_toc(uint8_t **outBuf, uint32_t *outLen, CdScsiError *outErr) {
    *outBuf = NULL;
    *outLen = 0;
    if (outErr) {
        memset(outErr, 0, sizeof(CdScsiError));
    }

    SInt32 score = 0;
    IOCFPlugInInterface **plugin = NULL;
    MMCDeviceInterface **mmc = NULL;
    SCSITaskDeviceInterface **dev = NULL;
    SCSITaskInterface **task = NULL;

    io_service_t devSvc = globalDevSvc;
    if (!devSvc && g_guard.bsdName) {
        (void)get_dev_svc(g_guard.bsdName);
        devSvc = globalDevSvc;
    }
    if (!devSvc) {
        fprintf(stderr, "[TOC] Could not find mmc device for bsd\n");
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

        fprintf(stderr, "[TOC] IOCreatePlugInInterfaceForService failed: 0x%x\n", kret);
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
    (void)hr;

    dev = (*mmc)->GetSCSITaskDeviceInterface(mmc);
    if (!dev) {
        fprintf(stderr, "GetSCSITaskDeviceInterface failed\n");
        goto fail;
    }

    // We need exclusive access so CreateSCSITask succeeds.
    kret = (*dev)->ObtainExclusiveAccess(dev);
    if (kret != kIOReturnSuccess) {
        if (kret == kIOReturnBusy) {
            fprintf(stderr, "[TOC] Busy on obtaining exclusive access\n");
        } else {
            fprintf(stderr, "[TOC] ObtainExclusiveAccess error: 0x%x\n", kret);
        }
        goto fail;
    }

    task = (*dev)->CreateSCSITask(dev);
    if (!task) {
        fprintf(stderr, "[TOC] CreateSCSITask failed\n");
        goto fail_excl;
    }

    const uint32_t alloc = 2048;
    uint8_t cdb[10] = {0};
    cdb[0] = 0x43; // READ TOC/PMA/ATIP
    cdb[1] = 0x00; // LBA format
    cdb[2] = 0x00; // Format 0x00: TOC
    cdb[6] = 0x00; // Starting track 0 = first track/session
    cdb[7] = (alloc >> 8) & 0xFF;
    cdb[8] = alloc & 0xFF;

    IOVirtualRange vr = {.address = 0, .length = 0};
    uint8_t *buf = malloc(alloc);
    if (!buf) {
        fprintf(stderr, "oom\n");
        goto fail_task;
    }
    vr.address = (IOVirtualAddress)buf;
    vr.length = alloc;

    if ((*task)->SetCommandDescriptorBlock(task, cdb, sizeof(cdb)) != kIOReturnSuccess) {
        fprintf(stderr, "SetCommandDescriptorBlock failed\n");
        goto fail_buf;
    }

    // 0 = no data, 1 = to device, 2 = from device
    if ((*task)->SetScatterGatherEntries(task, &vr, 1, alloc, 2) != kIOReturnSuccess) {
        fprintf(stderr, "SetScatterGatherEntries failed\n");
        goto fail_buf;
    }

    SCSI_Sense_Data sense = {0};
    SCSITaskStatus status = kSCSITaskStatus_No_Status;
    kern_return_t ex = (*task)->ExecuteTaskSync(task, &sense, &status, NULL);
    if (ex != kIOReturnSuccess || status != kSCSITaskStatus_GOOD) {
        fill_scsi_error(outErr, ex, status, &sense);
        fprintf(stderr, "ExecuteTaskSync failed (status=%u)\n", status);
        goto fail_buf;
    }

    *outBuf = buf;
    *outLen = alloc;

    (*task)->Release(task);
    (*dev)->ReleaseExclusiveAccess(dev);
    (*mmc)->Release(mmc);
    IODestroyPlugInInterface(plugin);
    return true;

fail_buf:
    free(buf);
fail_task:
    if (task) (*task)->Release(task);
fail_excl:
    if (dev) (*dev)->ReleaseExclusiveAccess(dev);
fail:
    if (mmc) (*mmc)->Release(mmc);
    if (plugin) IODestroyPlugInInterface(plugin);
    return false;
}

bool cd_read_toc(uint8_t **outBuf, uint32_t *outLen, CdScsiError *outErr) {
    return read_toc(outBuf, outLen, outErr);
}

void cd_free(void *p) {
    if (p) free(p);
}
