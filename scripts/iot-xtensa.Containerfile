# Xtensa toolchain image for hosts without a native espup toolchain (macOS
# Intel, where esp-rs stopped publishing x86_64-apple-darwin builds). Mirrors
# esp-rs/rust-build's idf-rust.Containerfile so the container gets the same
# rustc/cargo, GCC and LLVM versions espup installs natively.
#
# Built by scripts/iot-xtensa-image.sh; not used by CI.
FROM debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8

ARG CONTAINER_USER=esp
ARG ESP_BOARD=esp32s3
ARG XTENSA_VERSION=1.98.1.0
# 0.18.0 is yanked on crates.io; 0.17.1 is the last release upstream kept.
ARG ESPUP_VERSION=0.17.1

RUN apt-get -o Acquire::Retries=10 -o Acquire::http::Timeout=30 update \
    && apt-get -o Acquire::Retries=10 -o Acquire::http::Timeout=30 install -y --no-install-recommends \
      git curl ca-certificates gcc clang ninja-build unzip tar xz-utils \
      libudev-dev libusb-1.0-0 libssl-dev pkg-config \
      python3 python3-pip python3-venv libpython3-all-dev python3-virtualenv \
    && apt-get clean -y && rm -rf /var/lib/apt/lists/*

RUN adduser --disabled-password --gecos "" ${CONTAINER_USER}
USER ${CONTAINER_USER}
WORKDIR /home/${CONTAINER_USER}

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- \
      --default-toolchain none -y --profile minimal
ENV PATH=/home/${CONTAINER_USER}/.cargo/bin:${PATH}

RUN curl -L "https://github.com/esp-rs/espup/releases/download/v${ESPUP_VERSION}/espup-x86_64-unknown-linux-gnu" \
      -o "${HOME}/.cargo/bin/espup" \
    && chmod u+x "${HOME}/.cargo/bin/espup"

RUN espup install \
      --targets "${ESP_BOARD}" \
      --toolchain-version "${XTENSA_VERSION}" \
      --export-file "/home/${CONTAINER_USER}/export-esp.sh" \
    && echo "source /home/${CONTAINER_USER}/export-esp.sh" >> "${HOME}/.bashrc" \
    && rustup default esp

CMD [ "/bin/bash" ]
