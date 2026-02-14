#include "shim_common.h"

static Boolean read_toc(uint8_t **outBuf, uint32_t *outLen) {
    *outBuf = NULL;
    *outLen = 0;

    SInt32 score = 0;
    IOCFPlugInInterface **plugin = NULL;
    MMCDeviceInterface **mmc = NULL;
    SCSITaskDeviceInterface **dev = NULL;
    SCSITaskInterface **task = NULL;

    io_service_t devSvc = globalDevSvc;
    if (!devSvc) {
        fprintf(stderr, "[TOC] Could not find mmc device for bsd\n");
        goto fail;
    }

    kern_return_t kret = IOCreatePlugInInterfaceForService(
        devSvc,
        kIOMMCDeviceUserClientTypeID,
        kIOCFPlugInInterfaceID,
        &plugin,
        &score
    );

    if (kret != kIOReturnSuccess || plugin == NULL) {
        fprintf(stderr, "[TOC] IOCreatePlugInInterfaceForService failed: 0x%x\n", kret);
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
    if ((*task)->ExecuteTaskSync(task, &sense, &status, NULL) != kIOReturnSuccess
        || status != kSCSITaskStatus_GOOD) {
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

bool cd_read_toc(uint8_t **outBuf, uint32_t *outLen) {
    return read_toc(outBuf, outLen);
}

void cd_free(void *p) {
    if (p) free(p);
}
