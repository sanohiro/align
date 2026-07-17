# 21. State machines

> 🌐 **English** · [Japanese](./ja/21-state-machines.md)

In Chapter 19, we learned to unlearn objects. We stopped using instances with hidden `is_connected` booleans and internal mutable state. But how do we actually model a complex system—like a TCP connection, a game turn, or a UI component—without objects?

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

This struct can represent impossible states. What does it mean if `isConnected` is true, but `socketId` is null? What if `isAuthenticating` and `errorMessage` are both set? The compiler cannot help you here. You have to write tests to ensure these impossible states never happen.

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

Now, it is physically impossible to have a `user_id` while you are `Disconnected`. It is impossible to be `Authenticating` without a socket. The shape of the data perfectly matches the reality of the domain. (Payloads are positional, and today they are scalars or plain structs — when one deserves a name, give it a struct, e.g. `Connected(Session)`.)

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

## Why this is better

This is a **Finite State Machine**. By pulling the state out of a hidden object and representing it as explicit data, we gain several superpowers:

1. **Bug-proof:** `match` must cover every variant — the compiler rejects a missing arm. Every "ignore this event" is a wildcard *you wrote deliberately*, not a case you forgot.
2. **Testable:** Testing this logic does not require spinning up sockets or mocking objects. You just call `next(state, event)` and assert the output.
3. **Data-Oriented:** We can store an array of thousands of `ConnectionState` in memory and update them in bulk using a pipeline: `states.map(fn s { next(s, ev) }).to_array()`.

When you stop hiding state inside objects, your system becomes a pipeline of predictable transitions.
