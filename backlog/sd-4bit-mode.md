# Enable 4-bit bus mode for SD card

## Current state

The SD driver operates in 1-bit mode. The Host Control register bit 1
(DATA_TRANSFER_WIDTH) is never set, and ACMD6 is never sent to the card.

## Hardware capability

The DTB (`ref/nano.dtb`, node `cv-sd@4310000`) confirms:
- `bus-width = 4` — physically wired for 4-bit
- `cap-sd-highspeed` — high-speed mode supported
- `no-1-8-v` — 1.8V signaling not available, so UHS modes requiring it
  (SDR50, SDR104) are off the table; target is high-speed 4-bit at 3.3V
- `max-frequency = 25 MHz`

4-bit mode is a 4× throughput improvement at the same clock frequency with
no voltage switching complexity.

## What's required

Enabling 4-bit mode is a two-step handshake — both sides must agree:

1. Send CMD55 (APP_CMD) to prime the card for an application command
2. Send ACMD6 (SET_BUS_WIDTH) with argument `0x2` to switch the card to 4-bit
3. Set bit 1 of `REG_HOST_CONTROL` to switch the host controller to 4-bit

Both steps are required and must happen in order. Setting the host register
without telling the card (or vice versa) breaks communication.

## Complication: no card init sequence

The driver currently has no initialization sequence (no CMD0, CMD2, CMD3,
ACMD41). It relies on the bootloader to leave the card ready. Whether the
bootloader leaves the card in 1-bit or 4-bit mode is unknown — checking the
bootloader log or probing HOST_CONTROL at driver entry would clarify this.

If the bootloader already negotiated 4-bit mode, we may only need to set the
host control bit to match. If not, we need the full ACMD6 negotiation.

## When to act

Natural moment is when adding write support, since the initialization path
will be touched anyway. Also consider adding a proper card init sequence at
that point rather than relying on bootloader state.
