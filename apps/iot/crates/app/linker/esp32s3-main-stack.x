/* Extend the ESP32-S3 main task stack out of DRAM into DRAM2.
 *
 * `esp-hal`'s stack.x fills .stack from the top of the static data up to
 * `ORIGIN(RWDATA) + LENGTH(RWDATA)`, which stops at the start of dram2_seg and
 * leaves the 72 KiB there unclaimed. Drawing a render stamps a whole 240x320
 * frame while holding `Diagnostics` copies and two `[u16; ENVELOPE_COLUMNS]`
 * column arrays on the stack, and that ran the 41.5 KB stack into its floor:
 * the Audio page crashed with an exception whose A1 sat below `_stack_end`.
 *
 * dram2_seg is free once the second stage bootloader has handed over, and the
 * app links nothing into `.dram2_uninit`. Reserving it here keeps the stack
 * contiguous and hands the extra room to whoever reads the linker symbols:
 * the boot code loads its initial SP from `_stack_start_cpu0`, and esp-rtos
 * sizes the main task from `_stack_start_cpu0 - _stack_end_cpu0`.
 */

SECTIONS {
  /* NOLOAD, so this claims the addresses without adding anything to the image.
   * ALIGN(4) because the stack grows in words. */
  .stack_dram2 (NOLOAD) : ALIGN(4) {
    . = ORIGIN(dram2_seg);
    . = ORIGIN(dram2_seg) + LENGTH(dram2_seg);
  } > dram2_seg
}

/* esp-hal's stack.x already set these to the top of dram_seg. Reassigning them
 * to the top of dram2_seg is what makes the stack span both regions. ld takes
 * the last assignment, which is this one, because this script is linked after
 * linkall.x. */
_stack_start = ABSOLUTE(ORIGIN(dram2_seg) + LENGTH(dram2_seg));
_stack_start_cpu0 = ABSOLUTE(ORIGIN(dram2_seg) + LENGTH(dram2_seg));
