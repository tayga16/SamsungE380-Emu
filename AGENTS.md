# Samsung E830 Emulator

This is a standalone E830 device emulator extracted from the supplied source.
Only ARM firmware execution and the E830 board belong here. Keep the original
desktop interface layout, LCD scaling, fullscreen control and file picker.
Do not reintroduce Windows CE application loaders, game libraries or gamepads.

Read docs/boot-investigation.md before changing board behavior. Current native
Windows probes supersede the archived QEMU/forced-IRQ hypotheses. The real boot
splash is verified, but the main menu is not reached. IRQ 29 is DMA, IRQ 28 is
I2C; TMD_System_Clock is at 0x309011f0. NSF belongs to NOR at 0x21c20000 after
its NF4 wrapper; the retail TFS package is not a physical OneNAND dump. Do not
fake the menu, patch firmware, seed RTOS state or invent filesystem metadata.
Keep firmware and full RAM/NAND dumps out of public source packages.
The user specifies 32 MiB NAND. The current ec/14/version 0 profile follows
the firmware's 512 blocks, 64 pages/block and two 512-byte sectors/page.
Interleaved physical images use 1024+32 bytes/page; do not use the superseded
64 MiB ec/24 geometry. The exact fitted chip model remains unverified.

Use build-windows.ps1 for native Windows builds and -Test for tests. The CPU is
created and used inside its worker thread. Stop the worker before replacing it
or closing the app. Update investigation notes with evidence from new probes.
