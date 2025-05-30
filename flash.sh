#!/bin/sh
ELF_BINARY=${1}
rust-objcopy -O binary ${ELF_BINARY} ${ELF_BINARY}.bin
stm32flash -w ${ELF_BINARY}.bin -v -g 0x0 -b 115200 ${STM_SERIAL_PORT}
