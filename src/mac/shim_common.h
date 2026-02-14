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

typedef struct {
    uint8_t has_scsi_error;
    uint8_t scsi_status;
    uint8_t has_sense;
    uint8_t sense_key;
    uint8_t asc;
    uint8_t ascq;
    uint32_t exec_error;
    uint32_t task_status;
} CdScsiError;

extern DASessionRef g_session;
extern DAGuardCtx g_guard;
extern io_service_t globalDevSvc;

void start_da_guard(const char *bsdName);
void stop_da_guard(void);

bool cd_read_toc(uint8_t **outBuf, uint32_t *outLen, CdScsiError *outErr);
bool read_cd_audio(uint32_t lba, uint32_t sectors, uint8_t **outBuf, uint32_t *outLen, CdScsiError *outErr);
void cd_free(void *p);

Boolean get_dev_svc(const char *bsdName);
void reset_dev_scv(void);

#endif
