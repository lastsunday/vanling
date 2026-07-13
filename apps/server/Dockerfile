FROM --platform=$BUILDPLATFORM ubuntu:24.04
WORKDIR /app
ARG TARGETOS TARGETARCH
COPY chobits-server-${TARGETOS}-${TARGETARCH} ./server
RUN chmod +x ./server
RUN ./server downloader install --all
EXPOSE 3000
CMD ["./server"]
