

A minimal memory layout for STM32F407VG (adjust if your chip differs). This is a simple `memory.x` linker script to allow flashing.


```
MEMORY
{
FLASH : ORIGIN = 0x08000000, LENGTH = 1024K
RAM : ORIGIN = 0x20000000, LENGTH = 128K
}


_stack_start = ORIGIN(RAM) + LENGTH(RAM);