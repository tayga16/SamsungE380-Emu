# E830 late-menu HKBD input slice

Status: trace-backed partial pseudocode, 2026-11-02.  Firmware image and private
traces are intentionally omitted.

## Established path

The following is semantic pseudocode, not a line-for-line C reconstruction.
Unknown object fields and unverified branch conditions are deliberately left
unnamed.

```c
void hkbd_handle_matrix_event(uint16_t matrix_input, bool pressed)
{
    const KeyRecord *record = keypad_lookup(matrix_input);
    if (record == NULL)
        return;

    uint8_t key = record->semantic_key;

    if (pressed) {
        HkbdMessage *message = hkbd_build_press_message();
        message->key = key;

        /* The original firmware uses a separate payload group for ordinary
           digits and direction keys. */
        message->group = is_direction_key(key) ? 0 : record->group;
        hkbd_queue_message(0x2203, message);  /* 0x214d90c4 */
    } else {
        hkbd_queue_release_or_info(record);
    }
}
```

Observed values for the compared press paths:

| Input | Matrix value | Live record | Semantic key | Group | Message |
| --- | ---: | ---: | ---: | ---: | ---: |
| Digit `1` | `0x0801` | `0x30031994` | `0x01` | `0x03` | `0x2203` |
| Up | `0x0108` | `0x30031a44` | `0x14` | `0x00` | `0x2203` |

The initialized direction-key table gives `Up=0x14`, `Down=0x15`,
`Left=0x16`, and `Right=0x17`.

## Trace-backed address annotations

| Address | Annotation |
| ---: | --- |
| `0x208e51ac` | IRQ16 keypad handler entry observed for late input. |
| `0x2118bf80` | Keypad callback reached from the original IRQ path. |
| `0x20140be6` | Second live keypad-record lookup site. |
| `0x215a61ce` | HKBD press sender site; `Up` arrives as key `0x14`. |
| `0x214d90c4` | Common queue function called by digit and direction paths. |

## What this rules out

- The GUI-to-matrix mapping for Up is not missing: it produces `0x0108`.
- The live keypad table resolves that value to its initialized Up record.
- HKBD preserves semantic key `0x14` and creates press message `0x2203`.
- The direction event reaches the same queue function as a working digit event.

Therefore a host remap, sentinel key, synthetic callback or fabricated menu
event is not justified by current evidence.

## Next decompilation boundary

Start at the consumer of records inserted through `0x214d90c4`.  Compare the
working digit payload (`key=1`, `group=3`) with the ignored Up payload
(`key=0x14`, `group=0`) at these boundaries:

1. queue insertion and wake/signal operation;
2. receiving task selection and scheduler wake-up;
3. active-screen event classification;
4. callback/action-table dispatch;
5. redraw or state transition.

Do not infer a bad producer until the two records differ before insertion or
one fails to wake the original consumer.
