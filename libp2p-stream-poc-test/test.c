#include <stdio.h>
#include <unistd.h>
#include <pthread.h>
#include <string.h>
#include "artifact/libp2p_stream_poc.h"

void* read_thread_func(void *arg) {
    struct MirrorClient *mirror_client = (struct MirrorClient *)arg;

    while (1) {
        char buffer[1024];
        int result;
        if ((result = read_mirror_client(mirror_client, buffer, 0, 1024)) < 0) {
            printf("Error reading message: %d\n", result);
            break;
        }

        printf("Result of read: '%d`\n", result);

        buffer[result] = '\0';

        printf("Received message: '%s`\n", buffer);
    }

    printf("Exiting read thread\n");

    return NULL;
}

void* write_thread_func(void *arg) {
    struct MirrorClient *mirror_client = (struct MirrorClient *)arg;

    while (1) {
        char buffer[1024];

        if (fgets(buffer, sizeof buffer, stdin) == 0) {
            break;
        }

        printf("Sending message: '%s`\n", buffer);

        int len = strlen(buffer);
        int result;
        if ((result = write_mirror_client(mirror_client, buffer, 0, len)) < 0) {
            printf("Error writing message: %d\n", result);
            break;
        }
    }

    printf("Exiting write thread\n");

    return NULL;
}

int main(int argc, char **argv) {
    int result;

    if (argc < 2)  {
        printf("Usage: %s <peer>\n", argv[0]);
        return -1;
    }

    printf("Connecting to peer: %s\n", argv[2]);

    // init();
    struct NetworkContext *context = create_context(argv[1]);

    // sleep(10);

    struct MirrorClient *mirror_client;
    if ((result = connect_mirror(context, argv[2], &mirror_client)) < 0) {
        printf("Error connecting to peer: %d\n", result);
        return -1;
    }

    printf("Connected to peer\n");

    pthread_t read_thread;

    if (pthread_create(&read_thread, NULL, read_thread_func, mirror_client) != 0) {
        printf("Error creating read thread\n");
        return -2;
    }

    pthread_t write_thread;

    if (pthread_create(&write_thread, NULL, write_thread_func, mirror_client) != 0) {
        printf("Error creating write thread\n");
        return -3;
    }

    pthread_join(read_thread, NULL);
    pthread_join(write_thread, NULL);

    sleep(5);

    destroy_context(context);
}
