#include "shim_common.h"

static bool copy_bsd_name(io_service_t media, char *outName, size_t outNameLen) {
    if (!outName || outNameLen == 0) {
        return false;
    }

    CFTypeRef bsd = IORegistryEntryCreateCFProperty(
        media,
        CFSTR(kIOBSDNameKey),
        kCFAllocatorDefault,
        0
    );
    if (!bsd) {
        return false;
    }

    bool ok = false;
    if (CFGetTypeID(bsd) == CFStringGetTypeID()) {
        ok = CFStringGetCString((CFStringRef)bsd, outName, outNameLen, kCFStringEncodingUTF8);
    }

    CFRelease(bsd);
    return ok;
}

bool list_cd_drives(CdDriveInfo **outDrives, uint32_t *outCount) {
    if (!outDrives || !outCount) {
        return false;
    }

    *outDrives = NULL;
    *outCount = 0;

    CFMutableDictionaryRef match = IOServiceMatching(kIOCDMediaClass);
    if (!match) {
        return false;
    }

    io_iterator_t it = IO_OBJECT_NULL;
    kern_return_t kr = IOServiceGetMatchingServices(kIOMainPortDefault, match, &it);
    if (kr != KERN_SUCCESS) {
        return false;
    }

    CdDriveInfo *drives = NULL;
    uint32_t count = 0;
    io_service_t media;

    while ((media = IOIteratorNext(it))) {
        CdDriveInfo info;
        memset(&info, 0, sizeof(info));

        if (copy_bsd_name(media, info.bsd_name, sizeof(info.bsd_name))) {
            CdDriveInfo *next = realloc(drives, (count + 1) * sizeof(CdDriveInfo));
            if (!next) {
                IOObjectRelease(media);
                free(drives);
                IOObjectRelease(it);
                return false;
            }

            drives = next;
            drives[count] = info;
            count++;
        }

        IOObjectRelease(media);
    }

    IOObjectRelease(it);

    *outDrives = drives;
    *outCount = count;
    return true;
}
