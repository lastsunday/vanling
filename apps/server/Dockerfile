FROM --platform=$BUILDPLATFORM ubuntu:24.04
RUN apt update && apt install wget bzip2 -y
WORKDIR /app
ARG TARGETOS TARGETARCH
COPY chobits-server-${TARGETOS}-${TARGETARCH} ./server
COPY scripts/download.sh /app/download.sh
COPY scripts/download/ /app/download/
RUN chmod +x /app/download.sh
RUN sh /app/download.sh --all
EXPOSE 3000 
CMD ["./server"]
