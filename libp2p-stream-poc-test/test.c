#include <stdio.h>
#include <unistd.h>
#include "artifact/libp2p_stream_poc.h"

void callback(const char *msg) {
    printf("Received message: %s\n", msg);
}

void main(int argc, char **argv) {
    if (argc < 2)  {
        printf("Usage: %s <peer>\n", argv[0]);
        return;
    }

    init();
    struct NetworkContext *context = create_context(argv[1]);

    sleep(100);

    destroy_context(context);
}
