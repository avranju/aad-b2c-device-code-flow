FROM ubuntu:focal

# install dependencies
RUN apt update && \
    apt install ca-certificates libssl1.1 -y

WORKDIR /app

COPY aad-b2c-device-code-flow /app/aad-b2c-device-code-flow

EXPOSE 32468

CMD ["/app/aad-b2c-device-code-flow"]