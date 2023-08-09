# Saddle

A framework for avoiding the spooky-borrows-at-a-distance problem in Rust.

Sometimes, Rust's borrow checker is not intelligent enough to determine that code with valid borrowing semantics is actually valid, forcing us to reach for tools like `RefCell`. While `RefCell` is relatively easy to reason about in isolation, it can get really difficult to prove the validity of larger programs where `Refcell`s may be borrowed across dynamic dispatch boundaries. This is because the set of locations in code which could potentially cause a borrow conflict increases from the module's scope to, theoretically, the entire project's scope.

Saddle solves this by introducing the concept of **behavior namespaces**. Behaviors inside a behavior namespace statically define the set of object types they may be interested in borrowing as well as the behaviors they may be interested in dispatching to. Saddle then checks the program statically to ensure that it is impossible for a behavior to call another behavior which borrows an object type it may have already borrowed.

This mechanism strikes the balance between conservative-but-correct checking and the flexibility that is required from `RefCell`s in the first place. Individual modules—which have a much better time keeping track of their borrowing semantics by virtue of being small and self-contained—are able to borrow their own internal state freely. System glue code—where the issue of spooky-borrows-at-a-distance is known to manifest—are restricted to much more conservative borrowing patterns which provide actual guarantees of correctness.

If this ends up being too conservative, there's no issue! Users can create custom borrow helper objects which act like an individual module but can be passed across behavior namespace boundaries. This allows the user to define more flexible borrowing patterns while maintaining meaningful program structure.
