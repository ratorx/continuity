FROM alpine:latest
RUN apk add --no-cache curl bash iproute2
COPY start.sh .
ENTRYPOINT ["bash", "start.sh"]
