FROM ubuntu:24.04
WORKDIR /app
COPY data ./data
ARG TARGETOS TARGETARCH
COPY chobits-server-${TARGETOS}-${TARGETARCH} ./server
RUN chmod +x ./server
EXPOSE 3000
CMD ["./server"]
