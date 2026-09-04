# 21. State machines

> 🌐 **English** · [Japanese](./ja/21-state-machines.md)

Chapter 19 separated data from behavior. To represent a TCP connection, a game turn, or a UI component, we also need to describe its current state and the events that can change it.

In Align, we model state using **sum types** and **state machines**.

## The Problem with Booleans

Object-oriented code often represents state through a combination of fields:

```typescript
// Anti-pattern
class Connection {
    isConnected: boolean;
    isAuthenticating: boolean;
    socketId: number | null;
    errorMessage: string | null;
}
```

This struct can represent inconsistent states. What should happen if `isConnected` is true but `socketId` is null, or if `isAuthenticating` and `errorMessage` are both set? Code that updates these fields must keep them consistent, and tests must check those combinations.

## Making Invalid States Unrepresentable

In Align, we use a sum type to explicitly define exactly which states are possible, and we attach only the payload relevant to that specific state:

```align
ConnectionState {
    Disconnected,
    Connecting,
    Authenticating(i64),   // socket
    Connected(i64, i64),   // socket, user_id
    Failed(i64),           // error code
}
```

With this definition, `Disconnected` has no `user_id` field, and constructing `Authenticating` requires a socket. The type excludes those inconsistent combinations. Payloads are positional; when several values deserve one domain name, give them a struct, for example `Connected(Session)`. Borrowed text and supported owned arrays may also be payloads, with the ordinary region and Move rules from chapter [05](05-memory.md).

The `i64` socket values here are identifiers for a model, not owned OS socket handles. This example can copy a state because all payloads are Copy. If a state owns a real resource, transitions must transfer or borrow that ownership and arrange its cleanup; copying an identifier does not duplicate the resource.

## Transitions as Pure Functions

In OOP, state transitions happen when you call a method that mutates internal fields (`conn.connect()`). In Align, a transition is a pure function that takes the current state and an event, and returns the *next* state.

First, define the events that can happen:

```align
Event {
    Start,
    SocketOpened(i64),   // socket
    AuthSuccess(i64),    // user_id
    Failure(i64),        // error code
}
```

Then, write the transition function. `match` inspects one value at a time, so the function reads as a table: the outer `match` picks the row (the state), the inner one picks the column (the event).

```align
fn next(state: ConnectionState, event: Event) -> ConnectionState {
    return match state {
        Disconnected => match event {
            Start => ConnectionState.Connecting,
            Failure(code) => ConnectionState.Failed(code),
            _ => state,
        },
        Connecting => match event {
            SocketOpened(s) => ConnectionState.Authenticating(s),
            Failure(code) => ConnectionState.Failed(code),
            _ => state,
        },
        Authenticating(s) => match event {
            AuthSuccess(user_id) => ConnectionState.Connected(s, user_id),
            Failure(code) => ConnectionState.Failed(code),
            _ => state,
        },
        // Connected and Failed ignore further events (or you could return an error)
        _ => state,
    }
}
```

## Checking and testing transitions

This is a **finite state machine**. Representing it as data and a transition function gives us:

1. **Exhaustive cases:** `match` must cover every variant; the compiler rejects a missing arm. A wildcard explicitly handles any remaining cases.
2. **Testable:** Testing this logic does not require spinning up sockets or mocking objects. You just call `next(state, event)` and assert the output.
3. **Data-Oriented:** We can store an array of thousands of `ConnectionState` in memory and update them in bulk using a pipeline: `states.map(fn s { next(s, ev) }).to_array()`.

The transition logic can now be tested independently and applied to many states through the same pipeline.
