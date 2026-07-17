# 21. State machines

> 🌐 **English** · [Japanese](./ja/21-state-machines.md)

In Chapter 19, we learned to unlearn objects. We stopped using instances with hidden `is_connected` booleans and internal mutable state. But how do we actually model a complex system—like a TCP connection, a game turn, or a UI component—without objects?

In Align, we model state using **Sum Types (`enum`)** and **State Machines**.

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

This struct can represent impossible states. What does it mean if `isConnected` is true, but `socketId` is null? What if `isAuthenticating` and `errorMessage` are both true? The compiler cannot help you here. You have to write tests to ensure these impossible states never happen.

## Making Invalid States Unrepresentable

In Align, we use an `enum` to explicitly define exactly which states are possible, and we attach only the payload relevant to that specific state.

```align
ConnectionState {
    Disconnected,
    Connecting(url: string),
    Authenticating(socket: i64),
    Connected(socket: i64, user_id: i64),
    Failed(reason: string),
}
```

Now, it is physically impossible to have a `user_id` while you are `Disconnected`. It is impossible to be `Authenticating` without a `socket`. The shape of the data perfectly matches the reality of the domain.

## Transitions as Pure Functions

In OOP, state transitions happen when you call a method that mutates internal fields (`conn.connect()`). In Align, a transition is a pure function that takes the current state and an event, and returns the *next* state.

First, define the events that can happen:

```align
Event {
    Start(url: string),
    SocketOpened(socket: i64),
    AuthSuccess(user_id: i64),
    Error(reason: string),
}
```

Then, write the transition function. It is just a `match` on the current state and the event.

```align
fn next(state: ConnectionState, event: Event) -> ConnectionState {
    match (state, event) {
        // Happy paths
        (Disconnected, Start(url)) => 
            ConnectionState.Connecting(url),
            
        (Connecting(url), SocketOpened(socket)) => 
            ConnectionState.Authenticating(socket),
            
        (Authenticating(socket), AuthSuccess(user_id)) => 
            ConnectionState.Connected(socket, user_id),
            
        // Error handling
        (_, Error(reason)) => 
            ConnectionState.Failed(reason),
            
        // Invalid transitions are ignored (or you could return an error)
        _ => state
    }
}
```

## Why this is better

This is a **Finite State Machine**. By pulling the state out of a hidden object and representing it as explicit data, we gain several superpowers:

1. **Bug-proof:** The compiler checks that every state and event combination is handled (exhaustive matching).
2. **Testable:** Testing this logic does not require spinning up sockets or mocking objects. You just call `next(state, event)` and assert the output.
3. **Data-Oriented:** We can store an array of thousands of `ConnectionState` in memory (or a SoA) and update them in bulk using a pipeline: `states.map(fn s { next(s, ev) })`.

When you stop hiding state inside objects, your system becomes a pipeline of predictable transitions.
