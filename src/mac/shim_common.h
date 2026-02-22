#ifndef CD_DA_READER_MAC_SHIM_COMMON_H
#define CD_DA_READER_MAC_SHIM_COMMON_H

#import <CoreFoundation/CoreFoundation.h>
#import <IOKit/IOKitLib.h>
#import <IOKit/IOBSD.h>
#import <IOKit/storage/IOCDMediaBSDClient.h>
#import <IOKit/storage/IOCDMedia.h>
#import <IOKit/storage/IOCDTypes.h>

#include <stdbool.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

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

typedef struct {
    char bsd_name[64];
    uint8_t has_toc;
    uint8_t has_audio;
} CdDriveInfo;

extern char globalBsdName[64];

bool cd_read_toc(uint8_t **outBuf, uint32_t *outLen, CdScsiError *outErr);
bool read_cd_audio(uint32_t lba, uint32_t sectors, uint8_t **outBuf, uint32_t *outLen, CdScsiError *outErr);
bool read_cd_data(uint32_t lba, uint32_t sectors, uint8_t cdb_byte1, uint8_t cdb_byte9, uint32_t sector_size, uint8_t **outBuf, uint32_t *outLen, CdScsiError *outErr);
void cd_free(void *p);

bool list_cd_drives(CdDriveInfo **outDrives, uint32_t *outCount);

int open_cd_raw_device(void);
Boolean open_dev_session(const char *bsdName);
void close_dev_session(void);

#endif
