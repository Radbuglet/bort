# Safe Bort

Safe Bort is an opinionated subset of regular Bort's functionality which exchanges lower flexibility for a considerable improvement to borrow safety.

To achieve these guarantees, Safe Bort divides up the user's application into three parts:

- **Component modules**, which export sets of components implementing a specific application subsystem.
- **Behavior modules**, which export behavior handlers implementing entity behaviors. These associated behavior handlers are responsible for calling the necessary component handlers to make the application run.
- **Prefab modules**, which export constructors for specific entity types. These are generally defined side-by-side with their relevant behavior modules.

The list of component types a **component module** is allowed to access is defined statically and is generally just the list of components in that module. This implies that a component module is expected to not dynamically dispatch into other systems since doing so would require a very broad list of component types it almost certainly hasn't requested. To dynamically dispatch, therefore, a component module must take in event targets. Event targets are prefab-crafted handlers that either enqueue the dynamic dispatch for after the component yields control back to its calling behavior handler or carry it out immediately if doing so is safe.

Other than these access restrictions, Safe Bort is very flexible with respect to what is allowed in component modules. They are expected to be able to perform arbitrary computations and are generally allowed to do anything legal in regular Rust. This list of superpowers includes being able to dynamically borrow components without being beholden to the borrow checker. This is considered "safe" because, by virtue of disallowing dynamic dispatch, the scope of potential borrow issues decreases from an application-wide analysis to a mere per-module analysis, which is much more tenable.

**Behavior modules** are comprised of lists of functions implementing a specific behavior delegate type. These functions are free to call into any component methods they desire but must define both the component types they require access to and the behaviors they'll be executing during that time statically. Before the behavior registry can finish constructing, Safe Bort checks these relationships to ensure that it is impossible for a behavior to call into another behavior with a conflicting access requirement.

To summarize, whereas component modules are allowed to borrow only a fixed set of components in an unchecked fashion, behavior modules are allowed to borrow the full gamut of components in a very conservatively checked fashion.
