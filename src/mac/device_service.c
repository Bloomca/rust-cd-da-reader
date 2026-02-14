#include "shim_common.h"

io_service_t globalDevSvc = IO_OBJECT_NULL;
IOCFPlugInInterface **globalPlugin = NULL;
MMCDeviceInterface **globalMmc = NULL;
SCSITaskDeviceInterface **globalDev = NULL;

static io_service_t find_media(const char *bsdName) {
    io_iterator_t it = IO_OBJECT_NULL;
    io_service_t svc = IO_OBJECT_NULL;

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

    io_service_t cur;
    while ((cur = IOIteratorNext(it))) {
        if (IOObjectConformsTo(cur, kIOCDMediaClass)) {
            svc = cur;
            IOObjectRetain(svc);
            IOObjectRelease(cur);
            break;
        }

        io_service_t node = cur;
        IOObjectRetain(node);
        bool found_cd_media = false;

        for (int parent_depth = 0; node && parent_depth < 10; parent_depth++) {
            if (IOObjectConformsTo(node, kIOCDMediaClass)) {
                svc = node;
                IOObjectRetain(svc);
                found_cd_media = true;
                break;
            }

            io_iterator_t pit = IO_OBJECT_NULL;
            if (IORegistryEntryGetParentIterator(node, kIOServicePlane, &pit) != KERN_SUCCESS) {
                break;
            }

            io_service_t parent = IOIteratorNext(pit);
            IOObjectRelease(pit);
            IOObjectRelease(node);
            node = parent;
        }

        if (node) {
            IOObjectRelease(node);
        }
        IOObjectRelease(cur);

        if (found_cd_media) {
            break;
        }
    }

    IOObjectRelease(it);
    return svc;
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

// Climb until we find a node that lists the desired user client.
static io_service_t ascend_to_uc(io_service_t start, CFUUIDRef userClientType) {
    io_service_t node = start;
    IOObjectRetain(node);

    for (int depth = 0; node && depth < 32; depth++) {
        if (service_has_uc(node, userClientType)) return node;

        io_registry_entry_t parent = MACH_PORT_NULL;
        if (IORegistryEntryGetParentEntry(node, kIOServicePlane, &parent) != KERN_SUCCESS) {
            break;
        }

        IOObjectRelease(node);
        node = (io_service_t)parent;
    }

    if (node) IOObjectRelease(node);
    return IO_OBJECT_NULL;
}

Boolean get_dev_svc(const char *bsdName) {
    // Do not allow grabbing another drive while one is open.
    if (globalDevSvc) {
        return false;
    }

    io_service_t media = find_media(bsdName);
    if (!media) {
        fprintf(stderr, "[TOC] Could not find media for bsd\n");
        return false;
    }
    io_service_t devSvc = ascend_to_uc(media, kIOMMCDeviceUserClientTypeID);
    IOObjectRelease(media);

    if (!devSvc) {
        fprintf(stderr, "[TOC] Could not find mmc device for bsd\n");
        return false;
    }

    globalDevSvc = devSvc;
    return true;
}

void reset_dev_scv(void) {
    if (globalDevSvc) {
        IOObjectRelease(globalDevSvc);
    }
    globalDevSvc = IO_OBJECT_NULL;
}

Boolean open_dev_session(const char *bsdName) {
    if (globalDev) {
        return true;
    }

    if (!globalDevSvc && !get_dev_svc(bsdName)) {
        return false;
    }

    SInt32 score = 0;
    IOCFPlugInInterface **plugin = NULL;
    kern_return_t kret = IOCreatePlugInInterfaceForService(
        globalDevSvc,
        kIOMMCDeviceUserClientTypeID,
        kIOCFPlugInInterfaceID,
        &plugin,
        &score
    );
    if (kret != kIOReturnSuccess || !plugin) {
        fprintf(stderr, "[OPEN] IOCreatePlugInInterfaceForService failed: 0x%x\n", kret);
        return false;
    }

    MMCDeviceInterface **mmc = NULL;
    HRESULT hr = (*plugin)->QueryInterface(
        plugin,
        CFUUIDGetUUIDBytes(kIOMMCDeviceInterfaceID),
        (LPVOID)&mmc
    );
    if (hr != S_OK || !mmc) {
        fprintf(stderr, "[OPEN] QueryInterface(kIOMMCDeviceInterfaceID) failed (hr=0x%lx)\n", (long)hr);
        IODestroyPlugInInterface(plugin);
        return false;
    }

    SCSITaskDeviceInterface **dev = (*mmc)->GetSCSITaskDeviceInterface(mmc);
    if (!dev) {
        fprintf(stderr, "[OPEN] GetSCSITaskDeviceInterface failed\n");
        (*mmc)->Release(mmc);
        IODestroyPlugInInterface(plugin);
        return false;
    }

    kret = (*dev)->ObtainExclusiveAccess(dev);
    if (kret != kIOReturnSuccess) {
        if (kret == kIOReturnBusy) {
            fprintf(stderr, "[OPEN] Busy on obtaining exclusive access\n");
        } else {
            fprintf(stderr, "[OPEN] ObtainExclusiveAccess error: 0x%x\n", kret);
        }
        (*mmc)->Release(mmc);
        IODestroyPlugInInterface(plugin);
        return false;
    }

    globalPlugin = plugin;
    globalMmc = mmc;
    globalDev = dev;
    return true;
}

void close_dev_session(void) {
    if (globalDev) {
        (*globalDev)->ReleaseExclusiveAccess(globalDev);
        globalDev = NULL;
    }

    if (globalMmc) {
        (*globalMmc)->Release(globalMmc);
        globalMmc = NULL;
    }

    if (globalPlugin) {
        IODestroyPlugInInterface(globalPlugin);
        globalPlugin = NULL;
    }

    reset_dev_scv();
}
