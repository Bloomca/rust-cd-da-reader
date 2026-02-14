#include "shim_common.h"

DASessionRef g_session = NULL;
DAGuardCtx g_guard = {0};

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
    return NULL;
}

// Unmount completion: signal our waiter.
static void unmount_cb(DADiskRef disk, DADissenterRef dissenter, void *context) {
    DAGuardCtx *ctx = (DAGuardCtx *)context;
    (void)disk;
    (void)dissenter;
    dispatch_semaphore_signal(ctx->sem);
}

void start_da_guard(const char *bsdName) {
    g_session = DASessionCreate(kCFAllocatorDefault);
    if (!g_session) return;

    DASessionScheduleWithRunLoop(g_session, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);

    if (g_guard.bsdName) {
        free((void *)g_guard.bsdName);
        g_guard.bsdName = NULL;
    }
    g_guard.bsdName = strdup(bsdName);
    g_guard.sem = dispatch_semaphore_create(0);

    // Veto remounts while we run.
    DARegisterDiskMountApprovalCallback(g_session, NULL, mount_approval_cb, &g_guard);

    // Kick one unmount so the device is no longer busy.
    char path[64];
    snprintf(path, sizeof(path), "/dev/%s", bsdName);
    DADiskRef d = DADiskCreateFromBSDName(kCFAllocatorDefault, g_session, path);
    if (!d) return;

    DADiskUnmount(d, kDADiskUnmountOptionDefault, unmount_cb, &g_guard);

    // Wait for unmount while pumping the run loop.
    dispatch_time_t timeout = dispatch_time(DISPATCH_TIME_NOW, 30 * NSEC_PER_SEC);
    while (dispatch_semaphore_wait(g_guard.sem, DISPATCH_TIME_NOW) != 0) {
        CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, true);
        if (dispatch_time(DISPATCH_TIME_NOW, 0) > timeout) {
            printf("Unmount timeout!\n");
            break;
        }
    }

    CFRelease(d);
}

void stop_da_guard(void) {
    if (!g_session) return;

    DAUnregisterCallback(g_session, mount_approval_cb, &g_guard);
    DASessionUnscheduleFromRunLoop(g_session, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
    CFRelease(g_session);
    g_session = NULL;

    if (g_guard.bsdName) {
        free((void *)g_guard.bsdName);
        g_guard.bsdName = NULL;
    }
}
