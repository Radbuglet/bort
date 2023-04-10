# To-Do

Core improvements:

- [ ] Add helper methods to `CompSlot`
- [ ] Allow users to specify destructor components that get dropped first
- [ ] Implement `Obj`

Multi-threading improvements:

- [ ] Stop exposing components under the type token `Option<T>`
- [ ] Define `RawStorage`, which can be transformed into any of the storage variants
- [ ] Unify storage view types with traits
- [ ] Deduplicate storage code
- [ ] Implement `NRefCell` namespaces
- [ ] Improve `parallelize` performance
- [ ] Remove `MainThreadJail` in favor of `NRefCell` jailing
- [ ] Allow thread-local access to `ParallismSession`
- [ ] Expose debug labels on other threads?

ECS improvements:

- [ ] Implement `Archetype` and its associated helpers
- [ ] Implement a paralell executor
- [ ] Implement event queues
- [ ] Implement command buffers

Performance improvements:

- [ ] Optimize allocation
- [ ] Investigate codegen
- [ ] Allow blocks to be repurposed for other types

Safety improvements:

- [ ] Improve `Debug` implementations
- [ ] Improve panic messages
- [ ] Improve panic safety
- [ ] Document safety invariants in cell
- [ ] Write tests
- [ ] Write general documentation
