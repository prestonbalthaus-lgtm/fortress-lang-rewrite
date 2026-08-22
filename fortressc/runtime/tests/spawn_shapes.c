/*
 * Does the spawn queue satisfy the four corpus shapes, at the SHIM level,
 * before any compiler wiring exists?
 *
 * Each case below is one of ProjectFortress/tests/Spawn*.fss transcribed into
 * C against the same shims generated code will call. They are here rather than
 * in a .fss gate because the failure mode of every one of them is a HANG, and
 * a hang is cheapest to bisect with no compiler in the loop.
 *
 * THE ARGUMENT THESE FOUR MAKE TOGETHER is that no drain policy works and a
 * real concurrent context is required:
 *   spawn2  progress with NO join before the parent's spin
 *   spawn3  the child must NOT be run to completion at the spawn site
 *   spawn6  `ready()` must be false immediately after spawning long work
 *   spawn5  a value comes back through val()
 * spawn2 against spawn3+spawn6 is the contradiction; it has no inline solution.
 *
 *   cc -I runtime runtime/tests/spawn_shapes.c runtime/shims.c -lgc -lm -pthread \
 *      -o spawn-shapes
 *
 * Every case is run under an alarm, because a wrong answer here does not
 * return: it never returns.
 *
 * MUTATION TABLE, run against runtime/shims.c with the tree committed first.
 * Expectations are BASELINED FROM A PRE-MUTATION RUN, never written in.
 *
 *   no-runner            delete the fortress_runner_start() call    HUNG:spawn2
 *   ready-always-true    `int done = 1;`                          FAILED:spawn6
 *   no-steal             `if (0)` on the QUEUED branch of val()      HUNG:steal
 *   no-done-broadcast    delete the completion broadcast             HUNG:steal
 *
 * 4/4 refused. ONE DOCUMENTED ESCAPE, and it is right to escape:
 *
 *   runner-state-write   delete `t->state = RUNNING` in the runner     SURVIVED
 *
 * because that write is not load bearing. The runner UNLINKS the task from the
 * queue before running it, so val() looking for a QUEUED task to steal already
 * fails to find it and falls through to the wait. `ready()` compares against
 * DONE, so QUEUED and RUNNING are the same answer there. No assertion this
 * suite can make separates the two states, and inventing one would be testing
 * the field rather than the behaviour.
 *
 * `no-done-broadcast` was predicted at `join` and refuses one case EARLIER, at
 * `steal` -- which also joins. The prediction was wrong and the row is kept at
 * the measured value, not the intended one.
 *
 * THE `fortress_in_parallel` PIN IS NOT COVERED HERE. It only matters for a
 * `for` inside a spawned body, which needs the compiler. Its row belongs to the
 * spawn gate, not to this file, and saying so is better than a row that cannot
 * refuse.
 */
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

void fortress_runtime_init(void);
void *fortress_spawn(void *(*body)(void *), void *env);
void *fortress_thread_val(void *handle);
void fortress_thread_wait(void *handle);
int fortress_thread_ready(void *handle);
void fortress_thread_stop(void *handle);

/* The captured environment, exactly the shape the outliner builds: a
 * by-reference capture is the ADDRESS of the parent's storage. */
struct env {
    volatile long *x;
    volatile long *y;
    long count;
};

static int failures = 0;
static const char *current = "none";

static void on_alarm(int sig) {
    (void)sig;
    /* write(2), not printf: this runs in a signal handler. */
    const char *msg = "HUNG   ";
    ssize_t ignored = write(2, msg, strlen(msg));
    ignored += write(2, current, strlen(current));
    ignored += write(2, "\n", 1);
    (void)ignored;
    _exit(9);
}

static void ok(const char *name, int cond) {
    if (cond) {
        printf("ok    %s\n", name);
    } else {
        printf("FAIL  %s\n", name);
        failures++;
    }
}

/* ---- Spawn1: the child runs at all, and wait() sees it ---- */
static void *body_set_x(void *raw) {
    struct env *e = raw;
    *e->x = 1;
    return NULL;
}

static void case_spawn1(void) {
    volatile long x = 0;
    struct env e = {&x, NULL, 0};
    void *t = fortress_spawn(body_set_x, &e);
    fortress_thread_wait(t);
    ok("spawn1  wait() sees the child's store", x == 1);
    fortress_thread_stop(t);
}

/* ---- Spawn2: NO join before the parent spins. This is the case that
 *      forbids running a spawned body only at a join point. ---- */
static void *body_set_xy(void *raw) {
    struct env *e = raw;
    *e->x = 1;
    *e->y = 1;
    return NULL;
}

static void case_spawn2(void) {
    volatile long x = 0, y = 0;
    struct env e = {&x, &y, 0};
    void *t = fortress_spawn(body_set_xy, &e);
    while (x == 0) {
        /* the parent's spin, with nothing draining any queue */
    }
    ok("spawn2  the parent's spin terminates with no join", x == 1);
    ok("spawn2  both stores are visible", y == 1);
    fortress_thread_stop(t);
}

/* ---- Spawn3: the CHILD spins on a value the parent sets after the spawn.
 *      Running the body to completion at the spawn site is a hang. ---- */
static void *body_spin_until_x(void *raw) {
    struct env *e = raw;
    *e->y = 1;
    while (*e->x == 0) {
    }
    return NULL;
}

static void case_spawn3(void) {
    volatile long x = 0, y = 0;
    struct env e = {&x, &y, 0};
    void *t = fortress_spawn(body_spin_until_x, &e);
    x = 1;
    fortress_thread_wait(t);
    ok("spawn3  the child saw the parent's later store", y == 1);
    fortress_thread_stop(t);
}

/* ---- Spawn5: a value comes back ---- */
static void *body_count(void *raw) {
    struct env *e = raw;
    long sum = 0;
    for (long i = 0; i < e->count; i++) {
        sum++;
    }
    return (void *)(intptr_t)sum;
}

static void case_spawn5(void) {
    struct env e = {NULL, NULL, 10};
    void *t = fortress_spawn(body_count, &e);
    long got = (long)(intptr_t)fortress_thread_val(t);
    ok("spawn5  val() returns the body's value", got == 10);
    fortress_thread_stop(t);
}

/* ---- Spawn6: ready() is FALSE immediately after spawning long work.
 *      Running the body at the spawn site makes it true. ---- */
static void case_spawn6(void) {
    struct env e = {NULL, NULL, 200000000L};
    void *t = fortress_spawn(body_count, &e);
    int ready = fortress_thread_ready(t);
    ok("spawn6  ready() is false while the body is in flight", ready == 0);
    fortress_thread_stop(t);
}

/* ---- val() must still work when the runner is busy forever: the queued task
 *      is STOLEN onto the calling thread. Nothing in the corpus writes this and
 *      it is the property the steal path exists for. ---- */
static void *body_forever(void *raw) {
    struct env *e = raw;
    while (*e->x == 0) {
    }
    return NULL;
}

static void case_steal(void) {
    volatile long never = 0;
    struct env blocking = {&never, NULL, 0};
    void *hog = fortress_spawn(body_forever, &blocking);
    struct env work = {NULL, NULL, 7};
    void *t = fortress_spawn(body_count, &work);
    long got = (long)(intptr_t)fortress_thread_val(t);
    ok("steal   a queued task runs on the caller when the runner is busy",
       got == 7);

    /* RELEASE THE HOG AND JOIN IT, and the join is not tidiness. `never` is
     * this frame's stack. Returning while the hog still spins on &never leaves
     * it reading memory the NEXT case's frame reuses -- and it holds the only
     * runner, so the next case's task never leaves the queue. That is the
     * documented starvation hazard, reached by a leak in the test rather than
     * by a program, and it cost a hang in `join` to find. */
    never = 1;
    fortress_thread_val(hog);
    fortress_thread_stop(hog);
    fortress_thread_stop(t);
}

/* ---- THE BLOCKING JOIN, and it is here because a mutation SURVIVED without
 *      it. Removing `pthread_cond_broadcast(&fortress_spawn_done)` left every
 *      case above green: each one joins so soon after spawning that the task is
 *      still QUEUED, so val() STEALS it and runs it on the calling thread, and
 *      the condvar is never touched. Nothing exercised the wait path at all.
 *
 *      So this case forces the task to be RUNNING before the parent joins: the
 *      child publishes `started`, the parent spins for it, and only then
 *      releases the child and joins. That join cannot be a steal. ---- */
static void *body_started_then_wait(void *raw) {
    struct env *e = raw;
    *e->y = 1;                 /* started */
    while (*e->x == 0) {       /* held until the parent releases */
    }
    return (void *)(intptr_t)42;
}

static void case_join_running(void) {
    volatile long release = 0, started = 0;
    struct env e = {&release, &started, 0};
    void *t = fortress_spawn(body_started_then_wait, &e);
    while (started == 0) {
        /* the task is now RUNNING on the runner, not QUEUED */
    }
    release = 1;
    long got = (long)(intptr_t)fortress_thread_val(t);
    ok("join    val() on a RUNNING task blocks and then returns", got == 42);
    fortress_thread_stop(t);
}

struct case_entry {
    const char *name;
    void (*run)(void);
};

int main(void) {
    static const struct case_entry cases[] = {
        {"spawn1", case_spawn1}, {"spawn2", case_spawn2}, {"spawn3", case_spawn3},
        {"spawn5", case_spawn5}, {"spawn6", case_spawn6}, {"steal", case_steal},
        {"join", case_join_running},
    };
    fortress_runtime_init();
    signal(SIGALRM, on_alarm);
    for (size_t i = 0; i < sizeof cases / sizeof cases[0]; i++) {
        current = cases[i].name;
        alarm(20);
        cases[i].run();
        alarm(0);
    }
    printf("%s\n", failures == 0 ? "SPAWN SHAPES GREEN" : "SPAWN SHAPES FAILED");
    return failures == 0 ? 0 : 1;
}
