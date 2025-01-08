#include <stdio.h>
#include <unistd.h>
#include "artifact/libp2p_stream_poc.h"

void callback(const char *msg) {
    printf("Received message: %s\n", msg);
}

void main(void) {
    init();
    struct NetworkContext *context = create_context();
    on_receive(context, &callback);
    printf("Counter: %d\n", get_counter(context));
    increment_counter(context);
    printf("Counter: %d\n", get_counter(context));

    while (1) {
        update(context);
        sleep(1);
    }
    destroy_context(context);
}
