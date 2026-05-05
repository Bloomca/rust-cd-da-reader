#include "shim_common.h"

char globalBsdName[64] = {0};
SCSITaskDeviceInterface **globalDev = NULL;
static IOCFPlugInInterface **globalPlugIn = NULL;

int open_cd_raw_device(void) {
    if (globalBsdName[0] == '\0') {
        errno = ENODEV;
        return -1;
    }

    char path[96];
    if (strncmp(globalBsdName, "/dev/rdisk", 10) == 0) {
        snprintf(path, sizeof(path), "%s", globalBsdName);
    } else if (strncmp(globalBsdName, "/dev/disk", 9) == 0) {
        snprintf(path, sizeof(path), "/dev/r%s", globalBsdName + 5);
    } else if (strncmp(globalBsdName, "rdisk", 5) == 0) {
        snprintf(path, sizeof(path), "/dev/%s", globalBsdName);
    } else {
        snprintf(path, sizeof(path), "/dev/r%s", globalBsdName);
    }

    return open(path, O_RDONLY | O_NONBLOCK);
}

Boolean open_dev_session(const char *bsdName) {
    if (!bsdName || bsdName[0] == '\0') {
        return false;
    }

    if (globalBsdName[0] != '\0') {
        if (strcmp(globalBsdName, bsdName) == 0) {
            return true;
        }
        close_dev_session();
    }

    snprintf(globalBsdName, sizeof(globalBsdName), "%s", bsdName);

    CFMutableDictionaryRef matching = IOServiceMatching(kIOCDMediaClass);
    if (!matching) return false;

    CFStringRef cfBsdName = CFStringCreateWithCString(kCFAllocatorDefault, bsdName, kCFStringEncodingUTF8);
    if (!cfBsdName) {
        CFRelease(matching);
        return false;
    }
    CFDictionaryAddValue(matching, CFSTR(kIOBSDNameKey), cfBsdName);
    CFRelease(cfBsdName);

    io_service_t service = IOServiceGetMatchingService(kIOMainPortDefault, matching);
    if (!service) return false;

    io_service_t taskDevice = IO_OBJECT_NULL;
    io_service_t current = service;
    IOObjectRetain(current);

    while (current) {
        if (IOObjectConformsTo(current, "SCSITaskDevice")) {
            taskDevice = current;
            break;
        }
        io_service_t parent = IO_OBJECT_NULL;
        kern_return_t kr = IORegistryEntryGetParentEntry(current, kIOServicePlane, &parent);
        IOObjectRelease(current);
        if (kr != KERN_SUCCESS) {
            current = IO_OBJECT_NULL;
        } else {
            current = parent;
        }
    }

    if (!taskDevice) {
        IOObjectRelease(service);
        return false;
    }

    SInt32 score = 0;
    kern_return_t kr = IOCreatePlugInInterfaceForService(
        taskDevice,
        kSCSITaskDeviceUserClientTypeID,
        kIOCFPlugInInterfaceID,
        &globalPlugIn,
        &score
    );
    IOObjectRelease(taskDevice);
    IOObjectRelease(service);

    if (kr != KERN_SUCCESS || !globalPlugIn) {
        return false;
    }

    kr = (*globalPlugIn)->QueryInterface(
        globalPlugIn,
        CFUUIDGetUUIDBytes(kSCSITaskDeviceInterfaceID),
        (LPVOID *)&globalDev
    );

    if (kr != KERN_SUCCESS || !globalDev) {
        IODestroyPlugInInterface(globalPlugIn);
        globalPlugIn = NULL;
        return false;
    }

    return true;
}

void close_dev_session(void) {
    if (globalDev) {
        (*globalDev)->Release(globalDev);
        globalDev = NULL;
    }
    if (globalPlugIn) {
        IODestroyPlugInInterface(globalPlugIn);
        globalPlugIn = NULL;
    }
    globalBsdName[0] = '\0';
}
