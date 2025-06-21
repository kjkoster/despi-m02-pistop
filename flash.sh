#!/bin/sh
ELF_BINARY=${1}

# Extract the binary from the ELF file in the format that the board expects
rust-objcopy -O binary ${ELF_BINARY} ${ELF_BINARY}.bin

# Write the binary to the board via the serial port. If you experience any
# issues flashing, try lowering the baud rate from 921600 to 115200.
stm32flash -w ${ELF_BINARY}.bin -v -g 0x0 -b 921600 ${STM_SERIAL_PORT}
