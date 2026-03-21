# Filtered Message Receive - Future Enhancement

## Problem

When a thread needs to wait for a specific type of message while other message types may also arrive, the current `receive_message()` API is insufficient. It wakes on any message, which can cause thrashing when the thread needs to wait for a particular condition.

### Concrete Example: UART Writer

The uart_writer thread receives `UartWriterMessage` with variants:
- `WriteData(Vec<u8>)` - new data to transmit
- `BufferSpace` - signal that TX ring buffer has space

When the buffer is full, uart_writer blocks waiting for `BufferSpace`. However, other threads may continue sending `WriteData` messages, which wake uart_writer even though it can't make progress until buffer space is available.

Current workaround (Approach 1): Use a separate `BUFFER_SPACE_AVAILABLE` flag and direct wake mechanism, bypassing the message system for flow control signals.

## Proposed Enhancement: Filtered receive_message

### New API

```rust
/// Receive a message matching a filter predicate.
/// Messages that don't match remain in the inbox for later.
/// Blocks until a matching message arrives.
pub fn receive_message_filtered<T, F>(filter: F) -> T
where
    F: Fn(&T) -> bool,
{
    loop {
        let current = Thread::current();

        // Scan inbox for matching message
        for i in 0..current.inbox.len() {
            let msg = current.inbox.remove(i).unwrap();
            let typed_msg = unsafe { Box::from_raw(msg.data as *mut T) };

            if filter(&*typed_msg) {
                return *typed_msg;
            } else {
                // Re-box and put back at same position
                let ptr = Box::into_raw(typed_msg);
                current.inbox.insert(i, Message { sender: msg.sender, data: ptr as usize });
            }
        }

        // No matching message found, block and retry
        block_now();
    }
}
```

### Usage in uart_writer

```rust
// Wait specifically for BufferSpace, leaving WriteData messages queued
let _ = thread::receive_message_filtered(|msg: &UartWriterMessage| {
    matches!(msg, UartWriterMessage::BufferSpace)
});
```

### Thrashing Prevention (Future Optimization)

The above still has thrashing: `WriteData` messages wake the thread, it scans inbox, finds no `BufferSpace`, blocks again. To eliminate this, `send_message` would need to know whether to wake the thread based on message type.

This would require either:
- A numeric type tag on `Message` that senders set and `send_message` can check
- A thread-level "waiting for" descriptor that `send_message` consults
- Some way to extract the enum discriminant generically

This optimization adds complexity and can be deferred until thrashing becomes a measurable problem.

## When to Implement

Consider implementing this when:
- Multiple subsystems need filtered message reception
- The flag-based workaround (Approach 1) proliferates to other areas
- We want a unified abstraction for all inter-thread signaling

## Trade-offs vs Current Approach (Separate Flags)

**Filtered receive pros:**
- Unified mechanism - all signaling through messages
- General-purpose - reusable pattern
- No proliferation of static flags for different conditions

**Filtered receive cons:**
- More complex receive logic
- Potential thrashing without the optimization
- Re-boxing overhead for non-matching messages
