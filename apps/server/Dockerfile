FROM --platform=$BUILDPLATFORM ubuntu:24.04 AS downloader
WORKDIR /app
COPY chobits-server-linux-amd64 ./server
RUN chmod +x ./server && ./server downloader install --all

FROM ubuntu:24.04
WORKDIR /app
COPY --from=downloader /app/data ./data
COPY chobits-server-${TARGETOS}-${TARGETARCH} ./server
RUN chmod +x ./server
EXPOSE 3000
CMD ["./server"]
