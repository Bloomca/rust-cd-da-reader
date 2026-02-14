#ifndef CD_DA_READER_MAC_SHIM_COMMON_H
#define CD_DA_READER_MAC_SHIM_COMMON_H

#import <CoreFoundation/CoreFoundation.h>
#import <IOKit/IOKitLib.h>
#import <IOKit/IOCFPlugIn.h>
#import <IOKit/IOBSD.h>
#import <IOKit/storage/IOCDMedia.h>
#import <IOKit/scsi/SCSITaskLib.h>
#import <IOKit/scsi/IOSCSIMultimediaCommandsDevice.h>
#include <DiskArbitration/DiskArbitration.h>
#include <dispatch/dispatch.h>

#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    const char *bsdName;
    dispatch_semaphore_t sem;
} DAGuardCtx;

extern DASessionRef g_session;
extern DAGuardCtx g_guard;
extern io_service_t globalDevSvc;

void start_da_guard(const char *bsdName);
void stop_da_guard(void);

bool cd_read_toc(uint8_t **outBuf, uint32_t *outLen);
bool read_cd_audio(uint32_t lba, uint32_t sectors, uint8_t **outBuf, uint32_t *outLen);
void cd_free(void *p);

Boolean get_dev_svc(const char *bsdName);
void reset_dev_scv(void);

#endif
