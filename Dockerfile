FROM alpine AS base

ARG TARGETPLATFORM

WORKDIR /app

RUN apk update && apk add --no-cache \
    ca-certificates \
    tzdata \
    ffmpeg

COPY ./favkeep-Linux-*.tar.gz  ./targets/

RUN if [ "$TARGETPLATFORM" = "linux/amd64" ]; then \
    tar xzvf ./targets/favkeep-Linux-x86_64-musl.tar.gz  -C ./; \
    elif [ "$TARGETPLATFORM" = "linux/arm/v7" ]; then \
    tar xzvf ./targets/favkeep-Linux-armv7-musl.tar.gz  -C ./; \
    else \
    tar xzvf ./targets/favkeep-Linux-aarch64-musl.tar.gz  -C ./; \
    fi

RUN rm -rf ./targets && chmod +x ./favkeep

FROM scratch

WORKDIR /app

ENV LANG=zh_CN.UTF-8 \
    TZ=Asia/Shanghai \
    HOME=/app \
    RUST_BACKTRACE=1 \
    RUST_LOG=None,favkeep=info

COPY --from=base / /

ENTRYPOINT [ "/app/favkeep" ]

VOLUME [ "/app/.config/favkeep" ]
