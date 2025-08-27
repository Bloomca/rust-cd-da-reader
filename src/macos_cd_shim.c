#import <CoreFoundation/CoreFoundation.h>
#import <IOKit/IOKitLib.h>
#import <IOKit/IOCFPlugIn.h>
#import <IOKit/IOBSD.h>
#import <IOKit/storage/IOCDMedia.h>
#import <IOKit/scsi/SCSITaskLib.h>
#import <IOKit/scsi/IOSCSIMultimediaCommandsDevice.h>
#include <stdio.h>
#include <string.h>
#include <stdbool.h>
#include <DiskArbitration/DiskArbitration.h>
#include <dispatch/dispatch.h>

typedef struct {
    const char *bsdName;
    dispatch_semaphore_t sem;
} DAGuardCtx;

static DASessionRef g_session = NULL;
static DAGuardCtx g_guard = {0};

static Boolean disk_matches(DADiskRef disk, const char *bsdName) {
    CFDictionaryRef desc = DADiskCopyDescription(disk);
    if (!desc) return false;
    CFStringRef bsd = CFDictionaryGetValue(desc, kDADiskDescriptionMediaBSDNameKey);
    char name[256] = {0};
    Boolean match = (bsd && CFStringGetCString(bsd, name, sizeof(name), kCFStringEncodingUTF8)
                        && strcmp(name, bsdName) == 0);
    CFRelease(desc);
    return match;
}

// Mount-approval callback: veto mounts for our target disk while active.
static DADissenterRef mount_approval_cb(DADiskRef disk, void *context) {
    DAGuardCtx *ctx = (DAGuardCtx *)context;
    if (disk_matches(disk, ctx->bsdName)) {
        return DADissenterCreate(kCFAllocatorDefault, kDAReturnNotPermitted, CFSTR("reserved by app"));
    }
    return NULL; // allow others
}

// Unmount completion: signal our waiter.
static void unmount_cb(DADiskRef disk, DADissenterRef dissenter, void *context) {
    DAGuardCtx *ctx = (DAGuardCtx *)context;
    (void)disk; (void)dissenter;
    dispatch_semaphore_signal(ctx->sem);
}

// Returns a session you must keep alive; when finished, dereg + invalidate.
void start_da_guard(const char *bsdName) {
    g_session = DASessionCreate(kCFAllocatorDefault);
    if (!g_session) return;
    DAGuardCtx *outCtx = &g_guard;
    DASessionScheduleWithRunLoop(g_session, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);

    outCtx->bsdName = bsdName;
    outCtx->sem = dispatch_semaphore_create(0);

    // Veto remounts while we run.
    DARegisterDiskMountApprovalCallback(g_session, NULL, mount_approval_cb, outCtx);

    // Kick one unmount so the device is no longer “busy”.
    char path[64];
    snprintf(path, sizeof(path), "/dev/%s", bsdName);
    DADiskRef d = DADiskCreateFromBSDName(kCFAllocatorDefault, g_session, path);
    if (d) {
        DADiskUnmount(d, kDADiskUnmountOptionDefault, unmount_cb, outCtx);
        
        // Wait for unmount while pumping the run loop
        dispatch_time_t timeout = dispatch_time(DISPATCH_TIME_NOW, 30 * NSEC_PER_SEC); // 30 sec timeout
        while (dispatch_semaphore_wait(outCtx->sem, DISPATCH_TIME_NOW) != 0) {
            // Pump the run loop briefly to allow callbacks to be processed
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, true);
            
            // Check for timeout
            if (dispatch_time(DISPATCH_TIME_NOW, 0) > timeout) {
                printf("Unmount timeout!\n");
                break;
            }
        }

        CFRelease(d);
    }
}

void stop_da_guard() {
    if (!g_session) return;
    // Remove callbacks and unschedule
    DAUnregisterCallback(g_session, mount_approval_cb, &g_guard);
    DASessionUnscheduleFromRunLoop(g_session, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
    CFRelease(g_session);
}

static io_service_t find_media(const char *bsd);
static Boolean read_toc(const char *bsd, uint8_t **outBuf, uint32_t *outLen);

bool cd_read_toc(const char *bsdName, uint8_t **outBuf, uint32_t *outLen) {
    Boolean ok = read_toc(bsdName, outBuf, outLen);
    return ok;
}

void cd_free(void *p) { if (p) free(p); }

static io_service_t find_media(const char *bsdName) {
    io_iterator_t it = IO_OBJECT_NULL;
    io_service_t svc = IO_OBJECT_NULL;

    printf("[DEBUG] Looking for BSD name: %s\n", bsdName);

    CFMutableDictionaryRef match = IOBSDNameMatching(kIOMainPortDefault, 0, bsdName);
    if (!match) {
        printf("[ERROR] Failed at IOBSDNameMatching for %s\n", bsdName);
        return IO_OBJECT_NULL;
    }
    
    kern_return_t kr = IOServiceGetMatchingServices(kIOMainPortDefault, match, &it);
    if (kr != KERN_SUCCESS) {
        printf("[ERROR] Failed at IOServiceGetMatchingServices for %s (error: 0x%x)\n", bsdName, kr);
        return IO_OBJECT_NULL;
    }

    printf("[DEBUG] Got service iterator, looking for services...\n");
    
    io_service_t cur;
    int service_count = 0;
    while ((cur = IOIteratorNext(it))) {
        service_count++;
        printf("[DEBUG] Found service #%d, checking for CD media...\n", service_count);
        
        // Check if this service directly conforms to IOCDMedia
        if (IOObjectConformsTo(cur, kIOCDMediaClass)) {
            printf("[DEBUG] Service directly conforms to IOCDMedia!\n");
            svc = cur;
            IOObjectRetain(svc); // Retain since we're keeping it
            IOObjectRelease(cur); // Release our iterator reference
            break;
        }
        
        // Walk parents to find an IOCDMedia
        io_service_t node = cur;
        IOObjectRetain(node); // Retain for our parent walking
        bool found_cd_media = false;
        int parent_depth = 0;
        
        while (node && parent_depth < 10) { // Limit depth to prevent infinite loops
            char className[256];
            IOObjectGetClass(node, className);
            printf("[DEBUG] Checking parent at depth %d: %s\n", parent_depth, className);
            
            if (IOObjectConformsTo(node, kIOCDMediaClass)) {
                printf("[DEBUG] Found IOCDMedia at parent depth %d!\n", parent_depth);
                svc = node;
                IOObjectRetain(svc); // Retain since we're keeping it
                found_cd_media = true;
                break;
            }
            
            // Get parent
            io_iterator_t pit = IO_OBJECT_NULL;
            if (IORegistryEntryGetParentIterator(node, kIOServicePlane, &pit) != KERN_SUCCESS) {
                printf("[DEBUG] No more parents at depth %d\n", parent_depth);
                break;
            }
            
            io_service_t parent = IOIteratorNext(pit);
            IOObjectRelease(pit);
            IOObjectRelease(node); // Release current node
            node = parent; // Move to parent
            parent_depth++;
        }
        
        if (node) {
            IOObjectRelease(node); // Release the final node if we didn't keep it
        }
        IOObjectRelease(cur); // Release our iterator reference
        
        if (found_cd_media) {
            break;
        }
    }
    
    IOObjectRelease(it);
    
    if (svc) {
        printf("[SUCCESS] Found CD media service for %s\n", bsdName);
    } else {
        printf("[ERROR] Could not find CD media service for %s (checked %d services)\n", bsdName, service_count);
    }
    
    return svc;
}

static io_service_t find_mmc_device_for_bsd(const char *bsdName) {
    io_service_t media = find_media(bsdName);
    if (!media) {
        fprintf(stderr, "[MMC] no media for %s\n", bsdName);
        return IO_OBJECT_NULL;
    }

    io_service_t node = media;
    IOObjectRetain(node); // we will return this if it matches

    for (int depth = 0; node && depth < 32; depth++) {
        io_name_t cls = {0};
        if (IOObjectGetClass(node, cls) != KERN_SUCCESS) {
            fprintf(stderr, "[MMC] IOObjectGetClass failed at depth %d\n", depth);
            IOObjectRelease(node);
            IOObjectRelease(media);
            return IO_OBJECT_NULL;
        }
        fprintf(stderr, "[MMC] depth %d: %s\n", depth, cls);

        if (IOObjectConformsTo(node, "IOSCSIMultimediaCommandsDevice")) {
            fprintf(stderr, "[MMC] Found node which conforms to IOSCSIMultimediaCommandsDevice\n");
            // Found the device that vends the MMC/SCSI user client
            IOObjectRelease(media);    // drop original media
            return node;               // still has +1 retain
        }

        io_registry_entry_t parent = MACH_PORT_NULL;
        kern_return_t kr = IORegistryEntryGetParentEntry(node, kIOServicePlane, &parent);
        IOObjectRelease(node); // done with the current node either way

        if (kr != KERN_SUCCESS || parent == IO_OBJECT_NULL) {
            fprintf(stderr, "[MMC] Hit the root or error\n");
            // hit the root or error
            node = IO_OBJECT_NULL;
            break;
        }

        node = (io_service_t)parent; // parent comes retained
    }

    IOObjectRelease(media);
    return IO_OBJECT_NULL;
}

static bool service_has_uc(io_service_t svc, CFUUIDRef userClientType) {
    CFDictionaryRef d = IORegistryEntryCreateCFProperty(
        svc, CFSTR("IOCFPlugInTypes"), kCFAllocatorDefault, 0);
    if (!d) return false;
    CFStringRef want = CFUUIDCreateString(kCFAllocatorDefault, userClientType);
    bool ok = CFDictionaryContainsKey(d, want);
    CFRelease(want);
    CFRelease(d);
    return ok;
}

// climb until we find a node that lists the desired user client
static io_service_t ascend_to_uc(io_service_t start, CFUUIDRef userClientType) {
    io_service_t node = start; IOObjectRetain(node);
    for (int depth = 0; node && depth < 32; depth++) {
        if (service_has_uc(node, userClientType)) return node; // retained
        io_registry_entry_t parent = MACH_PORT_NULL;
        if (IORegistryEntryGetParentEntry(node, kIOServicePlane, &parent) != KERN_SUCCESS) break;
        IOObjectRelease(node);
        node = (io_service_t)parent; // retained
    }
    if (node) IOObjectRelease(node);
    return IO_OBJECT_NULL;
}


static Boolean read_toc(const char *bsdName, uint8_t **outBuf, uint32_t *outLen) {
    *outBuf = NULL; *outLen = 0;

    SInt32 score = 0;
    IOCFPlugInInterface **plugin = NULL;
    MMCDeviceInterface **mmc = NULL;
    SCSITaskDeviceInterface **dev = NULL;
    SCSITaskInterface **task = NULL;

    io_service_t media  = find_media(bsdName);
    io_service_t devSvc = ascend_to_uc(media, kIOMMCDeviceUserClientTypeID);
    fprintf(stderr, "[TOC] After finding device\n");

    if (!devSvc) {
        fprintf(stderr, "[TOC] Could not find mmc device for bsd\n"); goto fail;
    } else {
        fprintf(stderr, "[TOC] Found device successfully\n");
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
    fprintf(stderr, "[TOC] After calling IOCreatePlugInInterfaceForService\n");
    HRESULT hr = (*plugin)->QueryInterface(plugin, CFUUIDGetUUIDBytes(kIOMMCDeviceInterfaceID), (LPVOID)&mmc);
    fprintf(stderr, "[TOC] After calling QueryInterface\n");

    dev = (*mmc)->GetSCSITaskDeviceInterface(mmc);
    if (!dev) { fprintf(stderr, "GetSCSITaskDeviceInterface failed\n"); goto fail; }

    fprintf(stderr, "[TOC] Got non-null GetSCSITaskDeviceInterface\n");

    // We need to get exclusive access, otherwise `CreateSCSITask` will fail
    // in order to do so, we need to unmount the disk, claim it and make sure
    // we ignore register callbacks from other applications.
    // all of that needs to happen before this function is called
    kret = (*dev)->ObtainExclusiveAccess(dev);
    if (kret != kIOReturnSuccess) {
        if (kret == kIOReturnBusy) {
            fprintf(stderr, "[TOC] Busy on obtaining exclusive access"); goto fail;
        } else {
            fprintf(stderr, "[TOC] ObtainExclusiveAccess error: 0x%x\n", kret); goto fail;
        }
    }

    task = (*dev)->CreateSCSITask(dev);
    if (!task) { fprintf(stderr, "[TOC] CreateSCSITask failed\n"); goto fail_excl; }

    const uint32_t alloc = 2048; // technically, we need much less space, but for simplicity we get 2KB
    uint8_t cdb[10] = {0}; // TOC read command is 10 bytes
    cdb[0] = 0x43; // SCSI-2 command to read audio CD TOC (READ TOC/PMA/ATIP)
    cdb[1] = 0x00; // LBA format
    cdb[2] = 0x00; // Format 0x00: TOC
    cdb[6] = 0x00; // Starting track 0 = first track/session
    cdb[7] = (alloc >> 8) & 0xFF; // high byte of the buffer
    cdb[8] = alloc & 0xFF; // low byte of the buffer

    IOVirtualRange vr = { .address = 0, .length = 0 };
    uint8_t *buf = malloc(alloc);
    if (!buf) { fprintf(stderr, "oom\n"); goto fail_task; }
    vr.address = (IOVirtualAddress)buf; vr.length = alloc;

    if ((*task)->SetCommandDescriptorBlock(task, cdb, sizeof(cdb)) != kIOReturnSuccess) {
        fprintf(stderr, "SetCommandDescriptorBlock failed\n"); goto fail_buf;
    }
    // 0 = no data, 1 = to device, 2 = from device; SCSITaskLib uses these constants
    if ((*task)->SetScatterGatherEntries(task, &vr, 1, alloc, /*dir=*/2) != kIOReturnSuccess) {
        fprintf(stderr, "SetScatterGatherEntries failed\n"); goto fail_buf;
    }

    SCSI_Sense_Data sense = {0};
    SCSITaskStatus status = kSCSITaskStatus_No_Status;
    if ((*task)->ExecuteTaskSync(task, &sense, &status, NULL) != kIOReturnSuccess || status != kSCSITaskStatus_GOOD) {
        fprintf(stderr, "ExecuteTaskSync failed (status=%u)\n", status);
        goto fail_buf;
    }

    *outBuf = buf; *outLen = alloc;
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
    if (dev)  (*dev)->ReleaseExclusiveAccess(dev);
fail:
    if (mmc)  (*mmc)->Release(mmc);
    if (plugin) IODestroyPlugInInterface(plugin);
    return false;
}