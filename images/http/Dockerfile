FROM python:alpine
RUN apk add --no-cache iproute2
COPY start.sh .

ENTRYPOINT ["sh", "./start.sh"]
