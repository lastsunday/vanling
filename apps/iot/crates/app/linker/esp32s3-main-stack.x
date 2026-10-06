/* Extend the ESP32-S3 main task stack out of DRAM into DRAM2, stopping below the
 * ROM's per-core boot stacks.
 *
 * esp-hal's stack.x fills .stack from the top of the static data up to
 * ORIGIN(RWDATA) + LENGTH(RWDATA), which stops where dram2_seg begins and leaves
 * dram2_seg's 72 KiB unclaimed. A render stamps a whole 240x320 frame while holding
 * Diagnostics copies and two column arrays, and the stack that is left over runs into
 * its floor, so dram2_seg has to be available to it. The boot code loads its initial SP
 * from `_stack_start_cpu0` and esp-rtos sizes the main task from `_stack_start_cpu0 -
 * _stack_end_cpu0`, so moving that symbol up is what makes the stack span both regions.
 *
 * dram2_seg cannot be taken whole, because its top is the ROM's boot stack for each core:
 *
 *   0x3fce9710 - 0x3fceb710  PRO CPU stack
 *   0x3fceb710  - 0x3fced710  APP CPU stack
 *
 * `esp_rtos::start_second_core` boots the second core through the ROM, and the ROM runs
 * its startup prologue on the APP CPU stack before esp-hal's `start_core1_init` is
 * reached. A main stack reaching into that 16 KiB has its *top* overwritten, and the top
 * of the stack is where the thread-mode executor keeps its `ThreadFlag`, an ordinary local
 * in `Executor::run`'s frame: the first time that executor goes idle it dereferences the
 * cleared owner pointer and faults. What the second core runs is irrelevant, because the
 * damage is done before the closure is reached.
 *
 * esp-hal reserves these two stacks on the original ESP32, naming them
 * `reserved_rom_stack_pro` and `reserved_rom_stack_app`. It does not on the S3, whose
 * dram2_seg swallows them, so the reservation is made here.
 */

/* The ROM's PRO and APP CPU boot stacks, 8 KiB each, at the top of dram2_seg. */
ROM_BOOT_STACKS = 0x4000;

SECTIONS {
  /* NOLOAD, so this claims the addresses without adding anything to the image.
   * ALIGN(4) because the stack grows in words. The address expression is spelled
   * out rather than named: a symbol defined outside SECTIONS is not reliably
   * absolute when used as a location counter inside one. */
  .stack_dram2 (NOLOAD) : ALIGN(4) {
    . = ORIGIN(dram2_seg);
    . = ORIGIN(dram2_seg) + LENGTH(dram2_seg) - ROM_BOOT_STACKS;
  } > dram2_seg
}

/* esp-hal's stack.x already set these to the top of dram_seg. Reassigning them to
 * the top of dram2_seg less the ROM's stacks is what makes the stack span both
 * regions. ld takes the last assignment, which is this one, because this script is
 * linked after linkall.x. */
_stack_start = ABSOLUTE(ORIGIN(dram2_seg) + LENGTH(dram2_seg) - ROM_BOOT_STACKS);
_stack_start_cpu0 = ABSOLUTE(ORIGIN(dram2_seg) + LENGTH(dram2_seg) - ROM_BOOT_STACKS);

/* Pinned to the PRO CPU stack's low address. The stack's usable top and the ROM's
 * two stacks either side of it are one number, so asserting it here means a future
 * esp-hal that moves dram2_seg fails the link rather than quietly producing an
 * image whose main stack reaches into memory it must not. Re-derive the
 * reservation from the TRM's memory map when this trips. */
ASSERT(
  ORIGIN(dram2_seg) + LENGTH(dram2_seg) - ROM_BOOT_STACKS == 0x3FCE9710,
  "dram2_seg moved: re-derive the ROM boot stack reservation before the main stack uses its top"
)
