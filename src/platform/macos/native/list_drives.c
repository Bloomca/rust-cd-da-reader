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

static bool toc_has_audio_track(CFDataRef tocData) {
    if (!tocData) {
        return false;
    }

    CFIndex len = CFDataGetLength(tocData);
    if (len < (CFIndex)sizeof(CDTOC)) {
        return false;
    }

    CDTOC *toc = (CDTOC *)CFDataGetBytePtr(tocData);
    uint16_t tocLength = OSSwapBigToHostInt16(toc->length);
    size_t tocSize = (size_t)tocLength + sizeof(toc->length);
    if (tocSize > (size_t)len) {
        return false;
    }

    UInt32 count = CDTOCGetDescriptorCount(toc);
    for (UInt32 i = 0; i < count; i++) {
        CDTOCDescriptor *desc = &toc->descriptors[i];
        if (desc->adr != 1) {
            continue;
        }

        // Format 0x02 includes A0/A1/A2 metadata descriptors; real tracks are 1..99.
        if (desc->point < 1 || desc->point > 99) {
            continue;
        }

        if ((desc->control & 0x04) == 0) {
            return true;
        }
    }

    return false;
}

static bool inspect_toc(io_service_t media, uint8_t *hasToc, uint8_t *hasAudio) {
    if (hasToc) {
        *hasToc = 0;
    }
    if (hasAudio) {
        *hasAudio = 0;
    }

    CFTypeRef toc = IORegistryEntryCreateCFProperty(
        media,
        CFSTR(kIOCDMediaTOCKey),
        kCFAllocatorDefault,
        0
    );
    if (!toc) {
        return true;
    }

    if (CFGetTypeID(toc) == CFDataGetTypeID()) {
        if (hasToc) {
            *hasToc = 1;
        }
        if (hasAudio && toc_has_audio_track((CFDataRef)toc)) {
            *hasAudio = 1;
        }
    }

    CFRelease(toc);
    return true;
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
            inspect_toc(media, &info.has_toc, &info.has_audio);

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
