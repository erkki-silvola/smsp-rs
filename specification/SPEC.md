# SMSP Protocol Specification

## Overview

Simple Message Streaming Protocol (SMSP) is a lightweight protocol designed for IoT devices that provides reliable request/response communication with message fragmentation and flow control.

## Protocol Header

### Header Structure

```
Byte 0: [NNNN][R][L][SS]
├─ SS    = Status (bits 0-1, 2 bits)
│          00 = LAST/ACK
│          01 = MORE
│          10 = CANCEL/ERROR
│          11 = RESERVED
├─ L     = Length present flag (bit 2, 1 bit)
├─ R     = Reserved (bit 3, 1 bit)
└─ NNNN  = Sequence number (bits 4-7, 4 bits, 0-15)

Bytes 1-2 (optional): Payload length (big-endian u16, if length_present=1)
Bytes 3+: Payload data
```

### Header Fields

| Field | Bits | Size | Description |
|-------|------|------|-------------|
| `status` | 0-1 | 2 bits | Message status (LAST/ACK/MORE/CANCEL/RESERVED) |
| `length_present` | 2 | 1 bit | Indicates if length field is present (permanent per connection) |
| `reserved` | 3 | 1 bit | Reserved for future use |
| `seq` | 4-7 | 4 bits | Sequence number (0-15 with wraparound) |
| `length` | - | 16 bits | Payload length (only if length_present=1) |
| `payload` | - | Variable | Message payload |

## Connection Initialization

### Initialization Sequence

1. Connection initiator becomes the **master** (can only send requests)
2. Connection receiver becomes the **slave** (can only respond to requests)
3. Master sends initialization request: `"parameters?\0"`
4. Slave shall responds within 5 seconds with parameters

### Parameter Format

```
"<version>,<length_present>,<timeout_ms>,<fragment_count>,<fragment_max_size if length not present shall be 0>\n"
```

**Example:**
```
"1,1,20000,1,256\n"
```

**Field Description:**
- `version`: Protocol version (currently 1)
- `length_present`: 0 or 1, indicates if length field is used
- `timeout_ms`: Operation timeout in milliseconds
- `fragment_count`: Number of fragments to send before waiting for ACK (1-MAX::u8)
- `fragment_max_size`: Maximum fragment size in bytes (1-MAX::u16)

## Roles

### Master (Connection Initiator)
- Initiates requests
- Must follow parameters set by slave
- Cannot receive unsolicited messages
- Sends data in fragments with MORE/LAST status
- Waits for ACKs according to fragment_count

### Slave (Connection Receiver)
- Responds to requests only
- Responds to connection parameters
- Sends ACKs after receiving fragment_count fragments
- Can send CANCEL to abort operations

## Protocol Examples

### Example 1: Basic Request/Response

**Scenario:** Master sends 3 fragments, slave sends 3 fragment response (fragment_count=1)

```
┌─────────────────────┐                           ┌─────────────────────┐
│   Master (Host)     │                           │  Slave (IoT Device) │
└─────────────────────┘                           └─────────────────────┘
          │                                                   │
          │ ════════════ CONNECTION ESTABLISHED ════════════ │
          │                                                   │
          │  "parameters?\0"                                 │
          │ ─────────────────────────────────────────────────>│
          │                                                   │
          │                  "1,1,20000,1,256\n"             │
          │<───────────────────────────────────────────────── │
          │                                                   │
          │ ═══════ MASTER SENDS REQUEST(Sender) ════════════ │
          │                                                   │
          │  [MORE, seq=0, len=256, payload]                 │
          │ ─────────────────────────────────────────────────>│
          │                                                   │
          │                  [ACK, seq=0, len=0]             │
          │<───────────────────────────────────────────────── │
          │                                                   │
          │  [MORE, seq=1, len=256, payload]                 │
          │ ─────────────────────────────────────────────────>│
          │                                                   │
          │                  [ACK, seq=0, len=0]             │
          │<───────────────────────────────────────────────── │
          │                                                   │
          │  [LAST, seq=2, len=128, payload]                 │
          │ ─────────────────────────────────────────────────>│
          │                                                   │
          │                  [ACK, seq=0, len=0]             │
          │<───────────────────────────────────────────────── │
          │                                                   │
          │ ════════SLAVE SENDS RESPONSE(Sender) ═════════════│
          │                                                   │
          │                  [MORE, seq=0, len=256, payload] │
          │<───────────────────────────────────────────────── │
          │                                                   │
          │  [ACK, seq=0, len=0]                             │
          │ ─────────────────────────────────────────────────>│
          │                                                   │
          │                  [MORE, seq=1, len=256, payload] │
          │<───────────────────────────────────────────────── │
          │                                                   │
          │  [ACK, seq=0, len=0]                             │
          │ ─────────────────────────────────────────────────>│
          │                                                   │
          │                  [LAST, seq=2, len=128, payload] │
          │<───────────────────────────────────────────────── │
          │                                                   │
          │  [ACK, seq=0, len=0]                             │
          │ ─────────────────────────────────────────────────>│
          │                                                   │
```

**Message Details:**

Request Fragments:
```
Fragment 1: status=MORE(01), seq=0, length=256, payload=[256 bytes]
Fragment 2: status=MORE(01), seq=1, length=256, payload=[256 bytes]
Fragment 3: status=LAST(00), seq=2, length=128, payload=[128 bytes]
```
ACK Responses:
```
ACK: status=ACK(00), seq=0, length=0, payload=[]
```

### Example 2: Slave Responds with Error

**Scenario:** Master sends fragment, slave immediately cancels with error

```
┌─────────────────────┐                           ┌─────────────────────┐
│   Master (Host)     │                           │  Slave (IoT Device) │
└─────────────────────┘                           └─────────────────────┘
          │                                                   │
          │ ════════════ CONNECTION ESTABLISHED ════════════ │
          │                                                   │
          │  "parameters?\0"                                 │
          │ ─────────────────────────────────────────────────>│
          │                                                   │
          │                  "1,1,20000,1,256\n"             │
          │<───────────────────────────────────────────────── │
          │                                                   │
          │ ════════════ MASTER SENDS REQUEST ══════════════ │
          │                                                   │
          │  [MORE, seq=0, len=256, payload]                 │
          │ ─────────────────────────────────────────────────>│
          │                                                   │
          │          [CANCEL, seq=0, len=N, [..]]    │
          │<───────────────────────────────────────────────── │
          │                                                   │
          │              ⚠ ERROR - Operation Cancelled       │
          │                                                   │
```

### Example 3: Slave Accepts Multiple Fragments Before ACK

**Scenario:** Slave configured with fragment_count=2 (sends ACK every 2 fragments)

```
┌─────────────────────┐                           ┌─────────────────────┐
│   Master (Host)     │                           │  Slave (IoT Device) │
└─────────────────────┘                           └─────────────────────┘
          │                                                   │
          │ ════════════ CONNECTION ESTABLISHED ════════════ │
          │                                                   │
          │  "parameters?\0"                                 │
          │ ─────────────────────────────────────────────────>│
          │                                                   │
          │                  "1,1,20000,2,256\n"             │
          │                   (fragment_count=2)              │
          │<───────────────────────────────────────────────── │
          │                                                   │
          │ ════════════ MASTER SENDS REQUEST ══════════════ │
          │                                                   │
          │  [MORE, seq=0, len=256, payload]                 │
          │ ─────────────────────────────────────────────────>│
          │                                                   │
          │           (no ACK - fragment_count=2)             │
          │                                                   │
          │  [MORE, seq=1, len=256, payload]                 │
          │ ─────────────────────────────────────────────────>│
          │                                                   │
          │                  [ACK, seq=0, len=0]             │
          │<───────────────────────────────────────────────── │
          │            (ACK after 2 fragments)                │
          │                                                   │
          │  [MORE, seq=2, len=128, payload]                 │
          │ ─────────────────────────────────────────────────>│
          │                                                   │
          │                  [ACK, seq=0, len=0]             │
          │<───────────────────────────────────────────────── │
          │       (ACK immediately for LAST fragment)         │
          │                                                   │
```

**Key Points:**
- Slave waits for `fragment_count` fragments before sending ACK
- Reduces ACK overhead on reliable networks

## Sequence Number Behavior

### Wraparound
- Sequence numbers use 4 bits (0-15)
- After 15, wraps back to 0
- Both master and slave track separate sequences in sender role

### ACK Sequence
- ACK messages use seq=0

## Message Types

### Data Messages

| Status | Code | Usage | Description |
|--------|------|-------|-------------|
| `LAST` | `00` | End of transmission | Final fragment of request or response |
| `MORE` | `01` | Continuation | More fragments follow |

### Control Messages

| Status | Code | Usage | Description |
|--------|------|-------|-------------|
| `CANCEL` | `10` | Error/Abort | Operation cancelled, payload contains reason |
| `RESERVED` | `11` | Future use | Reserved for protocol extensions |

### ACK Message
- Uses `LAST` status with zero-length payload
- Sent by receiver to acknowledge fragment(s)
- Must respect `fragment_count` parameter

## Flow Control

### Fragment Count Rules

1. **fragment_count = 1**: ACK after every fragment
2. **fragment_count = N**: ACK after every N fragments

### Example Flow Control (fragment_count=3)

```
Fragment 1 (MORE) ─────> (no ACK)
Fragment 2 (MORE) ─────> (no ACK)
Fragment 3 (MORE) ─────> [ACK] <─── 3 fragments received
Fragment 4 (MORE) ─────> (no ACK)
Fragment 5 (MORE) ─────> (no ACK)
Fragment 6 (LAST) ─────> [no ACK]
**** Roles Switched ****
Fragment 1 (no ACK) <───── (MORE)
Fragment 2 (no ACK) <───── (MORE)
Fragment 3 (ACK) <───── [MORE] <─── 3 fragments received
Fragment 4 (no ACK) <───── (MORE)
Fragment 5 (no ACK) <───── (MORE)
Fragment 6 (no ACK) <───── [LAST]
...
```

## Timeout Handling

### Initialization Timeout
- Slave must respond to `"parameters?\0"` within **5 seconds**
- If timeout occurs, connection shall be terminated by the master.

### Operation Timeout
- Configured in slave's parameter response
- Applies to response reception and ACK waiting

### Timeout Behavior
- Master times out waiting for ACK. Connection shall be terminated.

## Error Handling

### Cancel/Error Conditions

Master can send CANCEL when:
- Long transmission needs to be stopped.
- Sequence number mismatch

Slave can send CANCEL when:
- Internal error occurs.
- Not enough resources to process the request.
- Sequence number mismatch

### Error Payload
- Payload contains application layer agreed protocol.

## Implementation Notes

### Length Present Flag
- Set during parameter negotiation
- Remains constant for connection lifetime
- When false: payload length inferred from transport
- When true: explicit 16-bit length field included

### Reserved Bits
- Must be set to 0 by sender
- Should be ignored by receiver
- Available for future protocol extensions

### Version Compatibility
- Current version: 1
- Implementations must validate version during init
- Version mismatch should result in connection refusal

## Protocol State Machine

### Master States
```
Uninitialized ──[connect]──> Negotiating ──[params received]──> Ready
     │                              │                              │
     └──────────────────────────────┴──[error]──> Error/Closed <──┘
```

### Slave States
```
Uninitialized ──[accept]──> Negotiating ──[params sent]──> Ready
     │                           │                            │
     └───────────────────────────┴──[error]──> Error/Closed <┘
```

## Best Practices

### For IoT Devices (Slave)
- Use small fragment_max_size (256-512 bytes) to fit in RAM
- Set reasonable timeout (20-30 seconds for iot device to be able to process request or  fragment)

### For Clients (Master)
- Respect slave's fragment parameters
- Validate sequence numbers for data integrity

### Performance Optimization
- Increase fragment_count on reliable networks
- Increase fragment_max_size for throughput

## Security Considerations

This protocol does not provide:
- Message integrity verification, this should be handled on next layer.
