FROM ubuntu:focal

# install dependencies
RUN apt update && \
    apt install ca-certificates libssl1.1 -y

WORKDIR /app
ADD . /app

EXPOSE 32468

CMD ["/app/aad-b2c-device-code-flow"]